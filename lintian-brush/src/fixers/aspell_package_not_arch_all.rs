use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use lazy_regex::regex_is_match;
use std::path::PathBuf;

/// Does this binary package name look like an aspell dictionary package?
///
/// Mirrors lintian's `^aspell-[a-z]{2}(?:-.*)?$` check in
/// `Lintian::Check::Fields::Architecture`.
fn is_aspell_dictionary(package: &str) -> bool {
    regex_is_match!(r"^aspell-[a-z]{2}(?:-.*)?$", package)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        if !is_aspell_dictionary(&package) {
            continue;
        }
        // lintian only looks at the first architecture; it reports the tag
        // unless that architecture is exactly "all". A package without an
        // Architecture field is not flagged.
        let arch = binary.as_deb822().get("Architecture");
        match arch.as_deref() {
            None | Some("all") => continue,
            Some(_) => {}
        }

        let issue = LintianIssue::binary_with_info(
            &package,
            "aspell-package-not-arch-all",
            Visibility::Warning,
            vec![],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "aspell dictionary package {} is not Architecture: all.",
                package
            ),
            format!("Set Architecture: all on package {}.", package),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package.clone(),
                },
                field: "Architecture".into(),
                value: "all".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
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
        format!("Set Architecture: all on package {}.", packages[0])
    } else {
        format!("Set Architecture: all on packages {}.", packages.join(", "))
    }
}

declare_detector! {
    name: "aspell-package-not-arch-all",
    tags: ["aspell-package-not-arch-all"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Architecture",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_is_aspell_dictionary() {
        assert!(is_aspell_dictionary("aspell-en"));
        assert!(is_aspell_dictionary("aspell-de"));
        assert!(is_aspell_dictionary("aspell-pt-br"));
        // Two-letter language code followed by a suffix.
        assert!(is_aspell_dictionary("aspell-en-gb"));

        // The bare tool, not a dictionary.
        assert!(!is_aspell_dictionary("aspell"));
        // Language code must be exactly two letters before the optional suffix.
        assert!(!is_aspell_dictionary("aspell-eng"));
        assert!(!is_aspell_dictionary("aspell-e"));
        // Uppercase is not matched (lintian uses [a-z]).
        assert!(!is_aspell_dictionary("aspell-EN"));
        // Unrelated packages.
        assert!(!is_aspell_dictionary("libaspell-dev"));
        assert!(!is_aspell_dictionary("myspell-en"));
    }

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

    fn write_control(contents: &str) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();
        fs::write(debian_dir.join("control"), contents).unwrap();
        temp_dir
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
    fn test_set_arch_all() {
        let temp_dir = write_control(
            "Source: aspell-en\n\nPackage: aspell-en\nArchitecture: any\nDescription: English dictionary for aspell\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Set Architecture: all on package aspell-en."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: aspell-en\n\nPackage: aspell-en\nArchitecture: all\nDescription: English dictionary for aspell\n",
        );
    }

    #[test]
    fn test_already_arch_all() {
        let original = "Source: aspell-en\n\nPackage: aspell-en\nArchitecture: all\nDescription: English dictionary for aspell\n";
        let temp_dir = write_control(original);
        let control_path = temp_dir.path().join("debian/control");

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_architecture_field() {
        // lintian does not flag a package with no Architecture field.
        let original =
            "Source: aspell-en\n\nPackage: aspell-en\nDescription: English dictionary for aspell\n";
        let temp_dir = write_control(original);

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_non_aspell_package() {
        let original = "Source: myspell-en\n\nPackage: myspell-en\nArchitecture: any\nDescription: English dictionary\n";
        let temp_dir = write_control(original);

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_aspell_tool_not_flagged() {
        // The aspell tool itself is arch-dependent and should not match.
        let original =
            "Source: aspell\n\nPackage: aspell\nArchitecture: any\nDescription: GNU Aspell spell-checker\n";
        let temp_dir = write_control(original);

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_multiple_aspell_packages() {
        let temp_dir = write_control(
            "Source: aspell-dicts\n\nPackage: aspell-en\nArchitecture: any\nDescription: English\n\nPackage: aspell-de\nArchitecture: amd64\nDescription: German\n",
        );
        let control_path = temp_dir.path().join("debian/control");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Set Architecture: all on packages aspell-de, aspell-en."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: aspell-dicts\n\nPackage: aspell-en\nArchitecture: all\nDescription: English\n\nPackage: aspell-de\nArchitecture: all\nDescription: German\n",
        );
    }
}
