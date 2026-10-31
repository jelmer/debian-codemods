use crate::declare_detector;
use crate::diagnostic::{Action, ChangelogAction, Diagnostic};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(version) = ws.current_version() else {
        return Ok(Vec::new());
    };
    if version.is_native() {
        return Ok(Vec::new());
    }
    let old_version = version.to_string();
    if !old_version.contains("dsfg") {
        return Ok(Vec::new());
    }
    let new_version = old_version.replace("dsfg", "dfsg");

    let issue = LintianIssue::source_with_info(
        "dfsg-version-misspelled",
        Visibility::Warning,
        vec![old_version.clone()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!("Version {} contains misspelled `dsfg`.", old_version),
        "Replace misspelled `dsfg` with `dfsg` in the package version.",
        vec![Action::Changelog(ChangelogAction::SetEntryVersion {
            file: PathBuf::from("debian/changelog"),
            version: old_version,
            new_version,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "dfsg-version-misspelled",
    tags: ["dfsg-version-misspelled"],
    triggers: [debian_workspace::Trigger::Changelog(
        debian_workspace::ChangelogAspect::Version,
    )],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_changelog(base: &Path, version: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            format!(
                "example ({}) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
                version,
            ),
        )
        .unwrap();
    }

    fn run_apply(base: &Path, version: &str) -> Result<crate::FixerResult, FixerError> {
        let v: Version = version.parse().unwrap();
        let adapter = DetectorImpl;
        let ws =
            debian_workspace::fs_workspace::FsWorkspace::new(base, Some("example".into()), Some(v));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn fixes_misspelled_version() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.2.3+dsfg1-1");

        run_apply(tmp.path(), "1.2.3+dsfg1-1").unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/changelog")).unwrap(),
            "example (1.2.3+dfsg1-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );
    }

    #[test]
    fn fixes_dot_separated_dsfg() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0.dsfg1-1");

        run_apply(tmp.path(), "1.0.dsfg1-1").unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/changelog")).unwrap(),
            "example (1.0.dfsg1-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        );
    }

    #[test]
    fn skips_correctly_spelled_dfsg() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.2.3+dfsg1-1");
        assert!(matches!(
            run_apply(tmp.path(), "1.2.3+dfsg1-1"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn skips_native_package() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0dsfg");
        assert!(matches!(
            run_apply(tmp.path(), "1.0dsfg"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn skips_version_without_dsfg() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.2.3-1");
        assert!(matches!(
            run_apply(tmp.path(), "1.2.3-1"),
            Err(FixerError::NoChanges)
        ));
    }
}
