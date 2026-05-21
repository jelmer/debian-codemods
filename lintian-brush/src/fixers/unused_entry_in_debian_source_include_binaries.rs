use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Component, Path, PathBuf};

/// Path of the file that lists binary files to keep in a `3.0 (quilt)`
/// source package.
const INCLUDE_BINARIES: &str = "debian/source/include-binaries";

/// An `include-binaries` entry that references a path no longer present.
struct UnusedEntry {
    /// 1-based line number, matching the position in lintian's pointer.
    position: usize,
    /// The trimmed entry text, as lintian reports it.
    entry: String,
    /// Byte range of the whole line, including its trailing newline.
    range: TextRange,
}

/// Whether `rel` resolves to an existing file or directory in `ws`.
///
/// A path counts as missing only when both lookups return a definitive
/// "not found". Any I/O error (a directory read as a file, a permission
/// problem) is treated as "present" so a stale-looking entry is only ever
/// dropped when we could positively prove the path is gone.
fn entry_missing(ws: &dyn Workspace, rel: &Path) -> bool {
    matches!(ws.read_file(rel), Ok(None)) && matches!(ws.list_dir(rel), Ok(None))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from(INCLUDE_BINARIES);
    let content = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(text) = std::str::from_utf8(&content) else {
        return Ok(Vec::new());
    };

    let mut unused: Vec<UnusedEntry> = Vec::new();
    let mut offset = 0usize;
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();

        // lintian skips blank lines and lines that *start* with '#'.
        let body = line.trim_end_matches(['\r', '\n']);
        if body.trim().is_empty() || body.starts_with('#') {
            continue;
        }
        let entry = body.trim();

        // Leave absolute paths and parent-directory traversal alone:
        // resolving those reaches outside the package tree, which is not
        // something this fixer should silently rewrite.
        let entry_path = Path::new(entry);
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            continue;
        }

        if entry_missing(ws, entry_path) {
            unused.push(UnusedEntry {
                position: idx + 1,
                entry: entry.to_string(),
                range: TextRange {
                    start: line_start,
                    end: offset,
                },
            });
        }
    }

    // Emit one diagnostic per stale entry, highest line first. The
    // resulting `ReplaceText` actions are then applied back-to-front, so
    // an earlier removal never shifts the byte offsets of a later one.
    Ok(unused
        .into_iter()
        .rev()
        .map(|u| {
            let issue = LintianIssue::source_with_info(
                "unused-entry-in-debian-source-include-binaries",
                Visibility::Info,
                vec![format!("{} [{}:{}]", u.entry, INCLUDE_BINARIES, u.position)],
            );
            Diagnostic::with_actions(
                issue,
                format!(
                    "{} lists {}, which is not present in the source tree.",
                    INCLUDE_BINARIES, u.entry
                ),
                format!("Remove unused entry {} from {}.", u.entry, INCLUDE_BINARIES),
                vec![Action::Filesystem(FilesystemAction::ReplaceText {
                    file: rel.clone(),
                    range: u.range,
                    replacement: String::new(),
                })],
            )
            .with_certainty(Certainty::Certain)
        })
        .collect())
}

/// Summarise the removals as a single line for the commit message.
fn describe(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    if fixed.len() == 1 {
        format!("Remove unused entry from {}.", INCLUDE_BINARIES)
    } else {
        format!(
            "Remove {} unused entries from {}.",
            fixed.len(),
            INCLUDE_BINARIES
        )
    }
}

declare_detector! {
    name: "unused-entry-in-debian-source-include-binaries",
    tags: ["unused-entry-in-debian-source-include-binaries"],
    triggers: [
        debian_workspace::Trigger::File("debian/source/include-binaries"),
    ],
    cost: crate::detector::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn write(base: &Path, rel: &str, content: &str) {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_all_entries_present() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "debian/logo.png\n",
        );
        write(tmp.path(), "debian/logo.png", "binary");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_unused_entry() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "debian/logo.png\ndebian/gone.png\n",
        );
        write(tmp.path(), "debian/logo.png", "binary");

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/include-binaries")).unwrap(),
            "debian/logo.png\n"
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
    }

    #[test]
    fn test_keeps_comments_and_blank_lines() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "# keep this comment\n\ndebian/gone.png\ndebian/logo.png\n",
        );
        write(tmp.path(), "debian/logo.png", "binary");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/include-binaries")).unwrap(),
            "# keep this comment\n\ndebian/logo.png\n"
        );
    }

    #[test]
    fn test_removes_multiple_unused_entries() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "debian/gone1.png\ndebian/logo.png\ndebian/gone2.png\n",
        );
        write(tmp.path(), "debian/logo.png", "binary");

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/include-binaries")).unwrap(),
            "debian/logo.png\n"
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);
    }

    #[test]
    fn test_entry_pointing_at_directory_is_kept() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "debian/blobs\n",
        );
        write(tmp.path(), "debian/blobs/data.bin", "binary");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_last_entry_without_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "debian/source/include-binaries",
            "debian/logo.png\ndebian/gone.png",
        );
        write(tmp.path(), "debian/logo.png", "binary");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/include-binaries")).unwrap(),
            "debian/logo.png\n"
        );
    }
}
