use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{FixerError, LintianIssue};
use debian_analyzer::editor::check_generated_file;
use std::path::{Path, PathBuf};

/// Length of trailing whitespace (excluding the newline) on a line ending
/// with `\n`. Returns 0 if there's nothing to strip.
fn trailing_ws_len(line: &[u8], strip_tabs: bool) -> usize {
    let Some(newline_pos) = line.iter().position(|&b| b == b'\n') else {
        return 0;
    };
    let mut end = newline_pos;
    while end > 0 {
        let prev = line[end - 1];
        if prev == b' ' || (strip_tabs && prev == b'\t') {
            end -= 1;
        } else {
            break;
        }
    }
    newline_pos - end
}

/// One issue + the byte range to replace and the replacement bytes.
struct Edit {
    issue: LintianIssue,
    range: TextRange,
    replacement: Vec<u8>,
}

fn collect_edits(
    abs_path: &Path,
    relative_path: &str,
    strip_tabs: bool,
    strip_trailing_empty_lines: bool,
    delete_new_empty_line: bool,
) -> Result<(Vec<Edit>, bool), FixerError> {
    let content = match std::fs::read(abs_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(e) => return Err(FixerError::from(e)),
    };

    let mut edits: Vec<Edit> = Vec::new();
    let mut offset = 0usize;
    let mut last_line_end = 0usize;
    for (line_idx, line) in content.split_inclusive(|&b| b == b'\n').enumerate() {
        let line_end = offset + line.len();
        let ws_len = trailing_ws_len(line, strip_tabs);
        if ws_len > 0 {
            let nl_offset = offset + line.len() - 1; // position of '\n'
            let issue = LintianIssue::source_with_info(
                "trailing-whitespace",
                vec![format!("[{}:{}]", relative_path, line_idx + 1)],
            );
            // If stripping leaves an empty line and we're asked to drop
            // such lines, swallow the newline too. Otherwise replace
            // just the trailing whitespace (preserving '\n').
            let drops_to_empty = delete_new_empty_line && (line.len() - ws_len - 1) == 0;
            let replacement = if drops_to_empty {
                Vec::new()
            } else {
                b"\n".to_vec()
            };
            edits.push(Edit {
                issue,
                range: TextRange {
                    start: nl_offset - ws_len,
                    end: line_end,
                },
                replacement,
            });
        }
        offset = line_end;
        last_line_end = offset;
    }

    let mut had_trailing_empty = false;
    if strip_trailing_empty_lines {
        // Find the byte offset where the trailing "\n\n…" run begins.
        let lines: Vec<&[u8]> = content.split_inclusive(|&b| b == b'\n').collect();
        let trailing_empty = lines.iter().rev().take_while(|l| **l == b"\n").count();
        if trailing_empty > 0 {
            had_trailing_empty = true;
            // Compute the start offset of the first trailing empty line.
            let mut empty_start = last_line_end;
            for line in lines.iter().rev().take(trailing_empty) {
                empty_start -= line.len();
            }
            let issue = LintianIssue::source_with_info(
                "trailing-whitespace",
                vec![format!("[{}:EOF]", relative_path)],
            );
            edits.push(Edit {
                issue,
                range: TextRange {
                    start: empty_start,
                    end: last_line_end,
                },
                replacement: Vec::new(),
            });
        }
    }

    Ok((edits, had_trailing_empty))
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let mut emit_for_file = |rel: PathBuf,
                             abs: &Path,
                             strip_tabs,
                             strip_eof,
                             delete_new_empty|
     -> Result<bool, FixerError> {
        let (edits, _) = collect_edits(
            abs,
            &rel.to_string_lossy(),
            strip_tabs,
            strip_eof,
            delete_new_empty,
        )?;
        if edits.is_empty() {
            return Ok(false);
        }
        // Diagnostics are emitted in source order (so the
        // Fixed-Lintian-Issues block reads top-down). Actions need
        // reverse-offset order though — the applier re-reads the file
        // for each ReplaceText, so applying the latest-offset edit
        // first keeps earlier-offset byte positions stable. Achieve
        // both by attaching all the actions to the first diagnostic.
        let mut sorted_edits = edits;
        let issues: Vec<LintianIssue> = sorted_edits.iter().map(|e| e.issue.clone()).collect();
        sorted_edits.sort_by(|a, b| b.range.start.cmp(&a.range.start));
        let mut actions: Vec<Action> = Vec::with_capacity(sorted_edits.len());
        for edit in sorted_edits {
            // ReplaceText carries a String; every replacement here is
            // empty or "\n", so UTF-8 conversion is always safe.
            let replacement = String::from_utf8(edit.replacement).map_err(|e| {
                FixerError::Other(format!("non-UTF-8 replacement in {}: {}", rel.display(), e))
            })?;
            actions.push(Action::Filesystem(FilesystemAction::ReplaceText {
                file: rel.clone(),
                range: edit.range,
                replacement,
            }));
        }
        for (i, issue) in issues.into_iter().enumerate() {
            let plan_actions = if i == 0 {
                std::mem::take(&mut actions)
            } else {
                Vec::new()
            };
            diagnostics.push(Diagnostic::with_actions(
                issue,
                "Trim trailing whitespace.",
                plan_actions,
            ));
        }
        Ok(true)
    };

    let changelog_abs = base_path.join("debian/changelog");
    if changelog_abs.exists() {
        emit_for_file(
            PathBuf::from("debian/changelog"),
            &changelog_abs,
            true,
            true,
            false,
        )?;
    }

    let rules_abs = base_path.join("debian/rules");
    if rules_abs.exists() {
        // For debian/rules, leave tabs alone — they're load-bearing.
        emit_for_file(
            PathBuf::from("debian/rules"),
            &rules_abs,
            false,
            true,
            false,
        )?;
    }

    let control_abs = base_path.join("debian/control");
    if control_abs.exists() {
        match check_generated_file(&control_abs) {
            Err(_) => {
                let debian_dir = base_path.join("debian");
                if let Ok(entries) = std::fs::read_dir(&debian_dir) {
                    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                    sorted.sort_by_key(|e| e.file_name());
                    let mut control_changed = false;
                    for entry in sorted {
                        let file_name = entry.file_name();
                        let name = file_name.to_string_lossy();
                        if !name.starts_with("control.")
                            || name.ends_with('~')
                            || name.ends_with(".m4")
                        {
                            continue;
                        }
                        if emit_for_file(
                            PathBuf::from(format!("debian/{}", name)),
                            &entry.path(),
                            true,
                            true,
                            true,
                        )? {
                            control_changed = true;
                        }
                    }
                    if control_changed {
                        emit_for_file(
                            PathBuf::from("debian/control"),
                            &control_abs,
                            true,
                            true,
                            true,
                        )?;
                    }
                }
            }
            Ok(()) => {
                emit_for_file(
                    PathBuf::from("debian/control"),
                    &control_abs,
                    true,
                    true,
                    true,
                )?;
            }
        }
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "file-contains-trailing-whitespace",
    tags: ["trailing-whitespace"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_trailing_ws_len_spaces() {
        assert_eq!(trailing_ws_len(b"hello  \n", true), 2);
    }

    #[test]
    fn test_trailing_ws_len_tabs() {
        assert_eq!(trailing_ws_len(b"hello\t\n", true), 1);
        assert_eq!(trailing_ws_len(b"hello\t\n", false), 0);
    }

    #[test]
    fn test_trailing_ws_len_mixed() {
        assert_eq!(trailing_ws_len(b"hello \t \n", true), 3);
    }

    #[test]
    fn test_file_strip_whitespace_control() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            b"Source: lintian-brush  \n\nPackage: lintian-brush\nDescription: Testing\n Test test\t\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read(&control).unwrap(),
            b"Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            b"Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_strip_trailing_empty_lines() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, b"Source: test\n\n\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(fs::read(&control).unwrap(), b"Source: test\n");
    }
}
