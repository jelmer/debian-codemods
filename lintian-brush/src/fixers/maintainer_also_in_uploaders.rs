use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();
    let (Some(maintainer), Some(uploaders)) =
        (paragraph.get("Maintainer"), paragraph.get("Uploaders"))
    else {
        return Ok(Vec::new());
    };

    let uploaders_list: Vec<String> = uploaders.split(',').map(|s| s.trim().to_string()).collect();
    if !uploaders_list.contains(&maintainer) {
        return Ok(Vec::new());
    }
    let new_uploaders: Vec<String> = uploaders_list
        .into_iter()
        .filter(|u| u != &maintainer)
        .collect();

    let action = if new_uploaders.is_empty() {
        Action::Deb822(Deb822Action::RemoveField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
        })
    } else {
        Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
            value: new_uploaders.join(", "),
        })
    };

    let issue = LintianIssue::source_with_info("maintainer-also-in-uploaders", vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Remove maintainer from uploaders.",
        vec![action],
    )])
}

declare_fixer! {
    name: "maintainer-also-in-uploaders",
    tags: ["maintainer-also-in-uploaders"],
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
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_maintainer_in_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: John Doe <john@example.com>, Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_maintainer_only_uploader() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_maintainer_not_in_uploaders() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test\nMaintainer: John Doe <john@example.com>\nUploaders: Jane Smith <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_uploaders_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: John Doe <john@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_multiple_uploaders_with_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: Bob <bob@example.com>\nUploaders: Alice <alice@example.com>, Bob <bob@example.com>, Charlie <charlie@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Remove maintainer from uploaders.");

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nMaintainer: Bob <bob@example.com>\nUploaders: Alice <alice@example.com>, Charlie <charlie@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }
}
