use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

/// Files lintian's debian/trailing-whitespace check inspects for a missing
/// trailing newline. Kept in sync with the `%PROHIBITED_TRAILS` hash in
/// `Lintian::Check::Debian::TrailingWhitespace`.
const CHECKED_FILES: &[&str] = &["debian/changelog", "debian/control", "debian/rules"];

/// Whether `content` triggers `no-newline-at-end`.
///
/// Lintian splits the file on `\n` keeping trailing empty fields and emits the
/// tag unless the last line is empty (i.e. the file ends with a newline). An
/// empty file has a single empty line and so never triggers. This reduces to:
/// the file is non-empty and its last byte is not `\n`.
fn missing_trailing_newline(content: &[u8]) -> bool {
    !content.is_empty() && content.last() != Some(&b'\n')
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in CHECKED_FILES {
        let path = Path::new(rel);
        let content = match ws.read_file(path)? {
            Some(c) => c,
            None => continue,
        };

        // Lintian only checks files it can decode as UTF-8.
        if std::str::from_utf8(&content).is_err() {
            continue;
        }

        if !missing_trailing_newline(&content) {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "no-newline-at-end",
            Visibility::Warning,
            vec![format!("[{}]", rel)],
        );

        let end = content.len();
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("{} does not end with a newline.", rel),
            "Add a newline at the end of the file.",
            vec![Action::Filesystem(FilesystemAction::ReplaceText {
                file: PathBuf::from(rel),
                range: TextRange { start: end, end },
                replacement: "\n".to_string(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "no-newline-at-end",
    tags: ["no-newline-at-end"],
    triggers: [
        debian_workspace::Trigger::File("debian/changelog"),
        debian_workspace::Trigger::File("debian/control"),
        debian_workspace::Trigger::File("debian/rules"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use tempfile::tempdir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    #[test]
    fn missing_newline_detection() {
        assert!(missing_trailing_newline(b"foo"));
        assert!(missing_trailing_newline(b"foo\nbar"));
        assert!(!missing_trailing_newline(b"foo\n"));
        assert!(!missing_trailing_newline(b"foo\nbar\n"));
    }

    #[test]
    fn empty_file_does_not_trigger() {
        // An empty file ends in nothing, but lintian treats it as a single
        // empty line and does not emit the tag.
        assert!(!missing_trailing_newline(b""));
    }

    #[test]
    fn trailing_newline_only_does_not_trigger() {
        assert!(!missing_trailing_newline(b"\n"));
    }

    #[test]
    fn detect_emits_append_action() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\nSection: misc").unwrap();

        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 1);
        let end = "Source: test\nSection: misc".len();
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Filesystem(FilesystemAction::ReplaceText {
                file: PathBuf::from("debian/control"),
                range: TextRange { start: end, end },
                replacement: "\n".to_string(),
            })
        );
    }

    #[test]
    fn apply_appends_newline() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, "Source: test\nSection: misc").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\nSection: misc\n"
        );
    }

    #[test]
    fn no_changes_when_newline_present() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\n").unwrap();

        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn non_utf8_file_skipped() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        // 0xff is not valid UTF-8; lintian skips such files.
        fs::write(debian.join("control"), [0xffu8]).unwrap();

        assert!(detect_in(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn handles_multiple_files() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test").unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f").unwrap();

        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 2);
    }
}
