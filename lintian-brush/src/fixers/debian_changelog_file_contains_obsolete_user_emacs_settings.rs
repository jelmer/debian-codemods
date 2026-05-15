use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rel = PathBuf::from("debian/changelog");
    let bytes = match ws.read_file(&rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    let package = ws.package().unwrap_or("");
    let re = Regex::new(r"add-log-mailing-address: .*\n").unwrap();

    // Collect matches in reverse order so byte offsets remain stable as
    // earlier ReplaceText actions are applied.
    let matches: Vec<_> = re.find_iter(&content).collect();
    if matches.is_empty() {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for mat in matches.iter().rev() {
        let line_number = content[..mat.start()].matches('\n').count() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-changelog-file-contains-obsolete-user-emacs-settings",
            Visibility::Warning,
            vec![format!(
                "[usr/share/doc/{}/changelog.Debian.gz:{}]",
                package, line_number
            )],
        );

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/changelog contains obsolete add-log-mailing-address emacs setting.",
                "Drop no longer supported add-log-mailing-address setting from debian/changelog.",
                vec![Action::Filesystem(FilesystemAction::ReplaceText {
                    file: rel.clone(),
                    range: TextRange {
                        start: mat.start(),
                        end: mat.end(),
                    },
                    replacement: "".into(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-changelog-file-contains-obsolete-user-emacs-settings",
    tags: ["debian-changelog-file-contains-obsolete-user-emacs-settings"],
    triggers: [debian_workspace::Trigger::Changelog(
        debian_workspace::ChangelogAspect::Body,
    )],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path, package: &str) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some(package.to_string()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_remove_add_log_mailing_address() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let changelog = debian.join("changelog");
        fs::write(&changelog,
            "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nadd-log-mailing-address: \"joe@example.com\"\nEnd:\n",
        ).unwrap();

        let result = run_apply(tmp.path(), "libjcode-perl").unwrap();
        assert_eq!(
            result.description,
            "Drop no longer supported add-log-mailing-address setting from debian/changelog."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert_eq!(
            fs::read_to_string(&changelog).unwrap(),
            "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nEnd:\n",
        );
    }

    #[test]
    fn test_no_add_log_mailing_address() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("changelog"),
            "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nEnd:\n",
        ).unwrap();

        assert!(matches!(
            run_apply(tmp.path(), "libjcode-perl"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_changelog_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "libjcode-perl"),
            Err(FixerError::NoChanges)
        ));
    }
}
