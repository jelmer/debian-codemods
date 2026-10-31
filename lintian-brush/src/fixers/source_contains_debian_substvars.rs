use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

fn is_substvars_name(name: &str) -> bool {
    name == "substvars" || name.ends_with(".substvars")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();
    for name in entries {
        if !is_substvars_name(&name) {
            continue;
        }
        let rel = PathBuf::from("debian").join(&name);
        let display = format!("debian/{name}");
        let issue = LintianIssue::source_with_info(
            "source-contains-debian-substvars",
            Visibility::Warning,
            vec![display.clone()],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!("{display} is present in the source tree."),
                format!("Remove {display} from the source tree."),
                vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "source-contains-debian-substvars",
    tags: ["source-contains-debian-substvars"],
    triggers: [
        debian_workspace::Trigger::File("debian/substvars"),
        debian_workspace::Trigger::Glob("debian/*.substvars"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_issues(base: &Path) -> Vec<Diagnostic> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        detect(&ws, &FixerPreferences::default()).unwrap()
    }

    #[test]
    fn test_removes_debian_substvars() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let substvars = debian.join("substvars");
        fs::write(&substvars, "misc:Depends=foo\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!substvars.exists());
        assert_eq!(
            result.description,
            "Remove debian/substvars from the source tree."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
    }

    #[test]
    fn test_removes_package_substvars() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let substvars = debian.join("mypkg.substvars");
        fs::write(&substvars, "misc:Depends=bar\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!substvars.exists());
        assert_eq!(
            result.fixed_lintian_tags(),
            vec!["source-contains-debian-substvars"]
        );
    }

    #[test]
    fn test_removes_multiple_substvars() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let a = debian.join("substvars");
        let b = debian.join("foo.substvars");
        let c = debian.join("bar.substvars");
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        fs::write(&c, "").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!a.exists());
        assert!(!b.exists());
        assert!(!c.exists());
    }

    #[test]
    fn test_issue_info_matches_lintian() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("substvars"), "misc:Depends=foo\n").unwrap();

        let diags = detect_issues(tmp.path());
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("source-contains-debian-substvars")
        );
        assert_eq!(issue.info.as_deref(), Some("debian/substvars"));
        assert_eq!(
            issue.to_string(),
            "source: source-contains-debian-substvars debian/substvars"
        );
    }

    #[test]
    fn test_issue_info_for_package_substvars() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("foo.substvars"), "").unwrap();

        let diags = detect_issues(tmp.path());
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(issue.info.as_deref(), Some("debian/foo.substvars"));
    }

    #[test]
    fn test_no_change_when_no_substvars() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_similarly_named_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("substvars.bak"), "").unwrap();
        fs::write(debian.join("mysubstvars"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(debian.join("substvars.bak").exists());
        assert!(debian.join("mysubstvars").exists());
    }

    #[test]
    fn test_is_substvars_name() {
        assert!(is_substvars_name("substvars"));
        assert!(is_substvars_name("foo.substvars"));
        assert!(is_substvars_name("libfoo-dev.substvars"));
        assert!(!is_substvars_name("substvars.bak"));
        assert!(!is_substvars_name("mysubstvars"));
        assert!(!is_substvars_name("control"));
    }
}
