use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, LintianIssue};
use std::path::{Path, PathBuf};

const SEP: char = '\t';

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debian_dir = base_path.join("debian");
    if !debian_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&debian_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut diagnostics = Vec::new();
    for entry in entries {
        let file_name_os = entry.file_name();
        let Some(file_name) = file_name_os.to_str() else {
            continue;
        };
        let Some(package_name) = file_name.strip_suffix(".linda-overrides") else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            package_name,
            "package-contains-linda-override",
            vec![format!("usr/share/linda/overrides/{}", package_name)],
        );

        let rel = PathBuf::from("debian").join(file_name);
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("file{}{}", SEP, file_name),
            vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut files: Vec<String> = fixed
        .iter()
        .filter_map(|d| {
            d.message
                .split_once(SEP)
                .filter(|(tag, _)| *tag == "file")
                .map(|(_, name)| name.to_string())
        })
        .collect();
    files.sort();
    files.dedup();
    format!("Remove obsolete linda overrides: {}", files.join(", "))
}

declare_fixer! {
    name: "package-contains-linda-override",
    tags: ["package-contains-linda-override"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
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
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
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
