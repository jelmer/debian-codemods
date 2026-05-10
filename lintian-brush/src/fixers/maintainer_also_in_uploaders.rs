use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let (Some(maintainer), Some(uploaders)) = (source.get("Maintainer"), source.get("Uploaders"))
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
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
        })
    } else {
        Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Uploaders".into(),
            value: new_uploaders.join(", "),
        })
    };

    let issue =
        LintianIssue::source_with_info("maintainer-also-in-uploaders", Visibility::Warning, vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Maintainer is also listed in Uploaders.",
        "Remove maintainer from uploaders.",
        vec![action],
    )])
}

declare_detector! {
    name: "maintainer-also-in-uploaders",
    tags: ["maintainer-also-in-uploaders"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Uploaders",
        },
    ],
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

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
