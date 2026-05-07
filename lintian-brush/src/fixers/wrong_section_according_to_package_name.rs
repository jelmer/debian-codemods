use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use lazy_regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

const CERTAINTY: Certainty = Certainty::Likely;

/// Get the name-to-section mappings from lintian data
fn get_name_section_mappings(
    lintian_data_path: Option<&Path>,
) -> Result<Vec<(Regex, String)>, std::io::Error> {
    let mappings_path = if let Some(path) = lintian_data_path {
        path.join("fields/name_section_mappings")
    } else {
        Path::new("/usr/share/lintian/data/fields/name_section_mappings").to_path_buf()
    };

    let content = fs::read_to_string(&mappings_path)?;
    let mut regexes = Vec::new();

    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if let Some((regex_str, section)) = line.split_once("=>") {
            let regex_str = regex_str.trim();
            let section = section.trim();

            match regex::Regex::new(regex_str) {
                Ok(regex) => {
                    regexes.push((regex, section.to_string()));
                }
                Err(e) => {
                    tracing::warn!(
                        "{}:{}: Invalid regex '{}': {}",
                        mappings_path.display(),
                        lineno + 1,
                        regex_str,
                        e
                    );
                    continue;
                }
            }
        }
    }

    Ok(regexes)
}

/// Find the expected section for a package name based on mappings
fn find_expected_section<'a>(regexes: &'a [(Regex, String)], name: &str) -> Option<&'a str> {
    for (regex, section) in regexes {
        if regex.is_match(name) {
            return Some(section);
        }
    }
    None
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let regexes = match get_name_section_mappings(preferences.lintian_data_path.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to load name-section mappings: {}", e);
            return Ok(Vec::new());
        }
    };

    let default_section = control.source().as_ref().and_then(|s| s.get("Section"));

    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let Some(expected_section) = find_expected_section(&regexes, &package_name) else {
            continue;
        };
        let current_section = binary
            .get("Section")
            .or_else(|| default_section.clone())
            .unwrap_or_default();
        if current_section == expected_section {
            continue;
        }

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "wrong-section-according-to-package-name",
            vec![format!("{} => {}", current_section, expected_section)],
        );

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!(
                    "Fix section for binary package {} ({} ⇒ {}).",
                    package_name, current_section, expected_section
                ),
                format!("Set Section for {} to {}.", package_name, expected_section),
                vec![Action::Deb822(Deb822Action::SetField {
                    file: PathBuf::from("debian/control"),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Section".into(),
                    value: expected_section.to_string(),
                })],
            )
            .with_certainty(CERTAINTY),
        );
    }

    Ok(diagnostics)
}

/// Aggregate per-binary section changes into one
/// "Fix sections for binary package A (X ⇒ Y), binary package B ..." line,
/// matching the historical wording.
fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    // Each diagnostic's per-issue message is
    // "Fix section for binary package P (X ⇒ Y).". Reuse the substring
    // between "binary package " and the trailing period to assemble the
    // aggregate.
    let parts: Vec<&str> = fixed
        .iter()
        .filter_map(|(d, _)| {
            d.message
                .strip_prefix("Fix section for ")
                .and_then(|s| s.strip_suffix('.'))
        })
        .collect();
    format!("Fix sections for {}.", parts.join(", "))
}

declare_detector! {
    name: "wrong-section-according-to-package-name",
    tags: ["wrong-section-according-to-package-name"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Section",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Section",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        prefs: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, prefs)
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        run_apply_with(base, &FixerPreferences::default())
    }

    #[test]
    fn test_no_control() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_fix_python_package_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nSection: libs\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert!(result.description.contains("python"));

        // Section is inserted after Architecture per BINARY_FIELD_ORDER.
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nSection: libs\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nSection: python\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_no_change_correct_section() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: test-pkg\nSection: python\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_dbg_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nSection: libs\nMaintainer: Test User <test@example.com>\n\nPackage: test-pkg-dbg\nArchitecture: all\nDescription: Debug symbols\n Debug symbols\n",
        )
        .unwrap();

        run_apply(base_path).unwrap();

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nSection: libs\nMaintainer: Test User <test@example.com>\n\nPackage: test-pkg-dbg\nArchitecture: all\nSection: debug\nDescription: Debug symbols\n Debug symbols\n",
        );
    }

    #[test]
    fn test_minimum_certainty_not_met() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            minimum_certainty: Some(Certainty::Certain),
            ..Default::default()
        };
        let err = run_apply_with(base_path, &prefs).unwrap_err();
        assert!(matches!(err, FixerError::NotCertainEnough(..)));
    }
}
