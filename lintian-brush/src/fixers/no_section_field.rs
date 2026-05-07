use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use lazy_regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Get the name-to-section mappings from lintian data
fn get_name_section_mappings(
    lintian_data_path: Option<&Path>,
) -> Result<Vec<(Regex, String)>, std::io::Error> {
    let mappings_path = if let Some(path) = lintian_data_path {
        path.join("fields/name_section_mappings")
    } else {
        Path::new("/usr/share/lintian/data/fields/name_section_mappings").to_path_buf()
    };

    let content = std::fs::read_to_string(&mappings_path)?;
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
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    if let Some(source) = control.source() {
        if source.as_deb822().contains_key("Section") {
            return Ok(Vec::new());
        }
    }

    let regexes = match get_name_section_mappings(preferences.lintian_data_path.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to load name-section mappings: {}", e);
            return Ok(Vec::new());
        }
    };

    // For each binary, determine its current or assigned section.
    struct BinaryInfo {
        package: String,
        line_no: usize,
        existing_section: Option<String>,
        assigned_section: Option<String>,
    }

    let mut binaries: Vec<BinaryInfo> = Vec::new();
    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        let p = binary.as_deb822();
        let existing_section = p.get("Section");
        let assigned_section = if existing_section.is_none() {
            find_expected_section(&regexes, &package).map(str::to_string)
        } else {
            None
        };
        binaries.push(BinaryInfo {
            package,
            line_no: p.line() + 1,
            existing_section,
            assigned_section,
        });
    }

    // The "effective" section of each binary, after applying assigned_section.
    let effective_sections: Vec<Option<String>> = binaries
        .iter()
        .map(|b| {
            b.assigned_section
                .clone()
                .or_else(|| b.existing_section.clone())
        })
        .collect();

    let unique_effective: HashSet<&str> = effective_sections
        .iter()
        .filter_map(|s| s.as_deref())
        .collect();
    let any_assigned = binaries.iter().any(|b| b.assigned_section.is_some());

    // Build diagnostics. Two cases:
    //  (a) all binaries share a single effective section → set Section on
    //      Source and remove from each binary that has it.
    //  (b) otherwise → set Section on each binary that needs assignment.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let source_line = control
        .source()
        .map(|s| s.as_deb822().line() + 1)
        .unwrap_or(1);

    if unique_effective.len() == 1 && !effective_sections.iter().any(Option::is_none) {
        let section = effective_sections[0].clone().unwrap();

        let _ = any_assigned;
        // Per-binary diagnostics first, so each binary's lintian issue is
        // recorded with the right line number.
        for b in &binaries {
            if let Some(assigned) = &b.assigned_section {
                let issue = LintianIssue::binary_with_info(
                    &b.package,
                    "recommended-field",
                    vec![format!(
                        "(in section for {}) Section [debian/control:{}]",
                        b.package, b.line_no
                    )],
                );
                diagnostics.push(
                    Diagnostic::with_actions(
                        issue,
                        format!("Section field is missing for binary {}.", b.package),
                        format!("Set Section for binary {}.", b.package),
                        vec![Action::Deb822(Deb822Action::SetField {
                            file: control_rel.clone(),
                            paragraph: ParagraphSelector::Binary {
                                package: b.package.clone(),
                            },
                            field: "Section".into(),
                            value: assigned.clone(),
                        })],
                    )
                    .with_certainty(Certainty::Certain),
                );
            }
        }

        // Source-level diagnostic that sets the shared section and removes
        // it from every binary (those that had it explicitly, plus those
        // that the per-binary diagnostics above just set).
        let mut source_actions: Vec<Action> = vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: "Section".into(),
            value: section.clone(),
        })];
        for b in &binaries {
            source_actions.push(Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: b.package.clone(),
                },
                field: "Section".into(),
            }));
        }
        let issue = LintianIssue::source_with_info(
            "recommended-field",
            vec![format!(
                "(in section for source) Section [debian/control:{}]",
                source_line
            )],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Section field is missing on source.",
                format!("Set Section to {} on source.", section),
                source_actions,
            )
            .with_certainty(Certainty::Certain),
        );
    } else {
        for b in &binaries {
            let Some(assigned) = &b.assigned_section else {
                continue;
            };
            let issue = LintianIssue::binary_with_info(
                &b.package,
                "recommended-field",
                vec![format!(
                    "(in section for {}) Section [debian/control:{}]",
                    b.package, b.line_no
                )],
            );
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!("Section field is missing for binary {}.", b.package),
                    format!("Set Section for binary {}.", b.package),
                    vec![Action::Deb822(Deb822Action::SetField {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: b.package.clone(),
                        },
                        field: "Section".into(),
                        value: assigned.clone(),
                    })],
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

/// The fixer can produce three end states, distinguishable from the
/// action stream alone:
///   1. Source got a Section AND at least one binary got a Section
///      (binaries' name-based assignments turned out to be the same →
///      hoisted to source).
///   2. Source got a Section AND no binary got a Section (binaries
///      already had matching explicit sections → just hoisted).
///   3. No source Section was set; binaries got individual Sections
///      (binaries had different name-based assignments).
fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut source_section_set = false;
    let mut binaries_with_section_set: Vec<String> = Vec::new();
    for action in actions {
        let Action::Deb822(Deb822Action::SetField {
            paragraph, field, ..
        }) = action
        else {
            continue;
        };
        if field != "Section" {
            continue;
        }
        match paragraph {
            ParagraphSelector::Source => source_section_set = true,
            ParagraphSelector::Binary { package } => {
                binaries_with_section_set.push(package.clone());
            }
            _ => {}
        }
    }
    binaries_with_section_set.sort();
    binaries_with_section_set.dedup();

    if source_section_set && !binaries_with_section_set.is_empty() {
        "Section field set in source based on binary package names.".to_string()
    } else if source_section_set {
        "Section field set in source stanza rather than binary packages.".to_string()
    } else {
        format!(
            "Section field set for binary packages {} based on name.",
            binaries_with_section_set.join(", ")
        )
    }
}

declare_detector! {
    name: "no-section-field",
    tags: ["recommended-field"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_control() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_source_already_has_section() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-pkg\nSection: libs\nMaintainer: Test User <test@example.com>\n\nPackage: test-pkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_set_section_on_binary() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Section field set in source based on binary package names."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nSection: python\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_move_section_to_source() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nSection: python\nDescription: Test package\n Test description\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Section field set in source stanza rather than binary packages."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nSection: python\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n",
        );
    }

    #[test]
    fn test_multiple_binaries_same_section() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n\nPackage: python3-testpkg-extra\nArchitecture: all\nDescription: Extra package\n Extra description\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Section field set in source based on binary package names."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nSection: python\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n\nPackage: python3-testpkg-extra\nArchitecture: all\nDescription: Extra package\n Extra description\n",
        );
    }

    #[test]
    fn test_multiple_binaries_different_sections() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nDescription: Test package\n Test description\n\nPackage: test-pkg-doc\nArchitecture: all\nDescription: Documentation\n Documentation\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Section field set for binary packages python3-testpkg, test-pkg-doc based on name."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-pkg\nMaintainer: Test User <test@example.com>\n\nPackage: python3-testpkg\nArchitecture: all\nSection: python\nDescription: Test package\n Test description\n\nPackage: test-pkg-doc\nArchitecture: all\nSection: doc\nDescription: Documentation\n Documentation\n",
        );
    }
}
