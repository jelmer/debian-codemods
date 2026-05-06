use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        if !package.starts_with("fonts-") && !package.starts_with("xfonts-") {
            continue;
        }
        let arch = binary.as_deb822().get("Architecture");
        if !matches!(arch.as_deref(), Some("all") | None) {
            continue;
        }
        if binary.as_deb822().get("Multi-Arch").is_some() {
            continue;
        }

        let issue =
            LintianIssue::binary_with_info(&package, "font-package-not-multi-arch-foreign", vec![]);
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("Set Multi-Arch: foreign on package {}.", package),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package.clone(),
                },
                field: "Multi-Arch".into(),
                value: "foreign".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[Diagnostic], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::SetField {
                paragraph: ParagraphSelector::Binary { package },
                ..
            }) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();

    if packages.len() == 1 {
        format!("Set Multi-Arch: foreign on package {}.", packages[0])
    } else {
        format!(
            "Set Multi-Arch: foreign on packages {}.",
            packages.join(", ")
        )
    }
}

declare_detector! {
    name: "font-package-not-multi-arch-foreign",
    tags: ["font-package-not-multi-arch-foreign"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
    fn test_add_multi_arch_foreign_to_font_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-blah\n\nPackage: fonts-blah\nArchitecture: all\nDescription: Test font package\n\nPackage: ttf-blah\nArchitecture: all\nDescription: Transition package\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on package fonts-blah."
        );

        // Multi-Arch goes after Architecture per BINARY_FIELD_ORDER. ttf-blah
        // is unaffected (it's not a fonts-/xfonts- package).
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: fonts-blah\n\nPackage: fonts-blah\nArchitecture: all\nMulti-Arch: foreign\nDescription: Test font package\n\nPackage: ttf-blah\nArchitecture: all\nDescription: Transition package\n",
        );
    }

    #[test]
    fn test_xfonts_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: xfonts-test\n\nPackage: xfonts-test\nArchitecture: all\nDescription: X font package\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on package xfonts-test."
        );
    }

    #[test]
    fn test_non_font_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: regular-package\n\nPackage: regular-package\nArchitecture: all\nDescription: Regular package\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_multi_arch() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: fonts-blah\n\nPackage: fonts-blah\nArchitecture: all\nMulti-Arch: foreign\nDescription: Test font package\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_non_all_architecture() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-blah\n\nPackage: fonts-blah\nArchitecture: amd64\nDescription: Test font package\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_font_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-collection\n\nPackage: fonts-foo\nArchitecture: all\nDescription: Foo font\n\nPackage: fonts-bar\nArchitecture: all\nDescription: Bar font\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        // Aggregate description. Note: alphabetical ordering -> bar, foo.
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on packages fonts-bar, fonts-foo."
        );

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: fonts-collection\n\nPackage: fonts-foo\nArchitecture: all\nMulti-Arch: foreign\nDescription: Foo font\n\nPackage: fonts-bar\nArchitecture: all\nMulti-Arch: foreign\nDescription: Bar font\n",
        );
    }
}
