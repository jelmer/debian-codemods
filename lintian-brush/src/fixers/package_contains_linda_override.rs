use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

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
    for file_name in entries {
        let Some(package_name) = file_name.strip_suffix(".linda-overrides") else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            package_name,
            "package-contains-linda-override",
            Visibility::Warning,
            vec![format!("usr/share/linda/overrides/{}", package_name)],
        );

        let rel = PathBuf::from("debian").join(&file_name);
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("Package contains obsolete linda override {}.", file_name),
            format!("Remove obsolete linda override {}.", file_name),
            vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut files: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Filesystem(FilesystemAction::Delete { file }) => file
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    files.sort();
    files.dedup();
    format!("Remove obsolete linda overrides: {}", files.join(", "))
}

declare_detector! {
    name: "package-contains-linda-override",
    tags: ["package-contains-linda-override"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/*.linda-overrides"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_remove_linda_overrides() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let p1 = debian.join("libhugs-cabal-bundled.linda-overrides");
        let p2 = debian.join("test-package.linda-overrides");
        fs::write(&p1, "Tag: foo\n").unwrap();
        fs::write(&p2, "Tag: bar\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!p1.exists());
        assert!(!p2.exists());
        assert_eq!(
            result.description,
            "Remove obsolete linda overrides: libhugs-cabal-bundled.linda-overrides, test-package.linda-overrides"
        );
    }

    #[test]
    fn test_no_change_when_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_linda_overrides() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "").unwrap();
        fs::write(debian.join("rules"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_single_linda_override() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let p = debian.join("single.linda-overrides");
        fs::write(&p, "Tag: x\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!p.exists());
        assert_eq!(
            result.description,
            "Remove obsolete linda overrides: single.linda-overrides"
        );
    }
}
