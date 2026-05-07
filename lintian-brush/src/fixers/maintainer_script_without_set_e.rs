use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

const SCRIPTS: &[&str] = &["preinst", "prerm", "postinst", "config", "postrm"];

const SHEBANG_OLD: &str = "#!/bin/sh -e\n";
const SHEBANG_NEW: &str = "#!/bin/sh\n";

/// Compute the byte offset and the text to insert at it for adding a
/// `set -e` line to a script that opens with `#!/bin/sh -e`. Returns
/// `None` when no fix is needed (the file isn't a candidate, or already
/// has `set -e`).
///
/// The mathematics here mirrors the original Python/Rust algorithm: walk
/// the script line by line starting at line 1, find the first
/// non-comment, non-blank line, and insert before the line one position
/// earlier. The leading newline of the inserted text gets a blank
/// separator inserted on either side; which side depends on whether
/// `lines[i-1]` (absolute) is already blank.
fn compute_insertion(content: &[u8]) -> Option<(usize, String)> {
    let lines: Vec<&[u8]> = content.split_inclusive(|&b| b == b'\n').collect();
    if lines.is_empty() || lines[0] != SHEBANG_OLD.as_bytes() {
        return None;
    }
    if lines.iter().any(|l| *l == b"set -e\n") {
        return None;
    }

    // Find first non-comment, non-blank line in lines[1..].
    let mut found = None;
    for (i, line) in lines[1..].iter().enumerate() {
        let trimmed: Vec<u8> = line
            .iter()
            .copied()
            .filter(|&b| b != b'\n' && b != b'\r')
            .collect();
        let is_comment = line.starts_with(b"#") && trimmed != b"#DEBHELPER#";
        let is_blank = line.iter().all(|&b| b == b'\n' || b == b'\r');
        if !is_comment && !is_blank {
            found = Some(i);
            break;
        }
    }
    let Some(i) = found else { return None };

    // Insertion point: absolute byte offset of `lines[i]`. (Note: that's
    // one element behind the matched line — the original code's slice
    // boundary is `lines[1..i]` for prefix, `lines[i..]` for suffix.)
    let insert_at: usize = lines[..i].iter().map(|l| l.len()).sum();

    let prev_line_blank = if i > 0 {
        lines[i - 1]
            .iter()
            .all(|&b| b == b'\n' || b == b'\r' || b == b' ' || b == b'\t')
    } else {
        false
    };
    let insert_text = if prev_line_blank {
        "set -e\n\n".to_string()
    } else {
        "\nset -e\n".to_string()
    };

    Some((insert_at, insert_text))
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics = Vec::new();
    for script_name in SCRIPTS {
        let rel = PathBuf::from("debian").join(script_name);
        let Some(content) = ws.read_file(&rel)? else {
            continue;
        };
        let Some((offset, insert_text)) = compute_insertion(&content) else {
            continue;
        };

        let issue = LintianIssue::source_with_info(
            "maintainer-script-without-set-e",
            vec![format!("[{}]", script_name)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Maintainer script passes -e on the shebang line.",
            "Use set -e rather than passing -e on the shebang-line.",
            vec![
                // Insert `set -e` first (offsets in the ORIGINAL file
                // are valid until any other edit shifts them); the
                // Substitute below then strips ` -e` from the shebang.
                Action::Filesystem(FilesystemAction::ReplaceText {
                    file: rel.clone(),
                    range: TextRange {
                        start: offset,
                        end: offset,
                    },
                    replacement: insert_text,
                }),
                Action::Filesystem(FilesystemAction::Substitute {
                    file: rel,
                    from: SHEBANG_OLD.into(),
                    to: SHEBANG_NEW.into(),
                }),
            ],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "maintainer-script-without-set-e",
    tags: ["maintainer-script-without-set-e"],
    triggers: [
        crate::workspace::Trigger::File("debian/preinst"),
        crate::workspace::Trigger::File("debian/prerm"),
        crate::workspace::Trigger::File("debian/postinst"),
        crate::workspace::Trigger::File("debian/config"),
        crate::workspace::Trigger::File("debian/postrm"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple_replacement() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let prerm = debian.join("prerm");
        fs::write(&prerm, "#!/bin/sh -e\n# Foo\n# bar\n\necho \"blah\"\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&prerm).unwrap(),
            "#!/bin/sh\n# Foo\n# bar\n\nset -e\n\necho \"blah\"\n",
        );
        assert_eq!(
            result.description,
            "Use set -e rather than passing -e on the shebang-line."
        );
    }

    #[test]
    fn test_with_debhelper_tag() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let prerm = debian.join("prerm");
        fs::write(
            &prerm,
            "#!/bin/sh -e\n# Foo\n\n#DEBHELPER#\n\n# bar\n\necho \"blah\"\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&prerm).unwrap(),
            "#!/bin/sh\n# Foo\n\nset -e\n\n#DEBHELPER#\n\n# bar\n\necho \"blah\"\n",
        );
    }

    #[test]
    fn test_no_change_when_already_has_set_e() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("prerm"), "#!/bin/sh\nset -e\n\necho \"blah\"\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_dash_e() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("prerm"), "#!/bin/sh\n\necho \"blah\"\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_scripts() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
