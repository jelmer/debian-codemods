use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::package_class::{is_meta_package, is_transitional};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

const TOOLCHAIN_PACKAGES: &[&str] = &["debhelper"];

fn provides_dh_sequence(value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    let found = relations.entries().any(|e| {
        e.relations().any(|r| {
            r.try_name()
                .as_deref()
                .map(|n| n.starts_with("dh-sequence-"))
                .unwrap_or(false)
        })
    });
    found
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
        let Some(pkg_name) = binary.name() else {
            continue;
        };

        // Skip helpers that legitimately depend on toolchain packages.
        if pkg_name.starts_with("dh-")
            || pkg_name.ends_with("-source")
            || pkg_name.ends_with("-src")
        {
            continue;
        }

        let description = binary.get("Description").unwrap_or_default();
        let section = binary.get("Section");
        if is_transitional(&description)
            || is_meta_package(&pkg_name, &description, section.as_deref())
        {
            continue;
        }

        // Packages that are themselves debhelper add-ons (Provides:
        // dh-sequence-foo) may legitimately depend on debhelper.
        let provides = binary.as_deb822().get("Provides").unwrap_or_default();
        if provides_dh_sequence(&provides) {
            continue;
        }

        for field in DEP_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            let (relations, _errors) = Relations::parse_relaxed(&value, true);
            for toolchain in TOOLCHAIN_PACKAGES {
                let mentions = relations.entries().any(|e| {
                    e.relations()
                        .any(|r| r.try_name().as_deref() == Some(*toolchain))
                });
                if !mentions {
                    continue;
                }

                let issue = LintianIssue::binary_with_info(
                    &pkg_name,
                    "binary-package-depends-on-toolchain-package",
                    Visibility::Warning,
                    vec![format!("{}: {}", field, toolchain)],
                );
                diagnostics.push(
                    Diagnostic::with_actions(
                        issue,
                        format!(
                            "Binary package {} depends on toolchain package {}.",
                            pkg_name, toolchain
                        ),
                        format!(
                            "Drop unnecessary dependency on toolchain package {}.",
                            toolchain
                        ),
                        vec![Action::Deb822(Deb822Action::DropRelation {
                            file: control_rel.clone(),
                            paragraph: ParagraphSelector::Binary {
                                package: pkg_name.clone(),
                            },
                            field: (*field).to_string(),
                            package: (*toolchain).into(),
                        })],
                    )
                    .with_certainty(Certainty::Possible),
                );
            }
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "binary-package-depends-on-toolchain-package",
    tags: ["binary-package-depends-on-toolchain-package"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Recommends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Suggests",
        },
    ],
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

    fn write_control(content: &str) -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, content).unwrap();
        (tmp, control)
    }

    #[test]
    fn test_drops_debhelper_from_depends() {
        let (tmp, control) = write_control(
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: any\nDepends: debhelper, libc6\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: any\nDepends: libc6\nDescription: test\n test\n",
        );
    }

    #[test]
    fn test_drops_debhelper_from_recommends() {
        let (tmp, control) = write_control(
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: any\nDepends: libc6\nRecommends: debhelper (>= 13)\nDescription: test\n test\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: any\nDepends: libc6\nDescription: test\n test\n",
        );
    }

    #[test]
    fn test_no_change_without_toolchain() {
        let (tmp, _) = write_control(
            "Source: test-pkg\n\nPackage: test-pkg\nArchitecture: any\nDepends: libc6\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skips_dh_prefixed_package() {
        let (tmp, _) = write_control(
            "Source: dh-foo\n\nPackage: dh-foo\nArchitecture: all\nDepends: debhelper\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skips_source_suffix_package() {
        let (tmp, _) = write_control(
            "Source: foo\n\nPackage: foo-source\nArchitecture: all\nDepends: debhelper\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skips_transitional_package() {
        let (tmp, _) = write_control(
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDepends: debhelper\nDescription: transitional dummy\n This is a transitional package, it can be safely removed.\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skips_metapackage() {
        let (tmp, _) = write_control(
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDepends: debhelper\nDescription: foo metapackage\n A metapackage pulling in foo bits.\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_skips_dh_sequence_provider() {
        let (tmp, _) = write_control(
            "Source: dh-foo\n\nPackage: dh-foo-runtime\nArchitecture: all\nDepends: debhelper\nProvides: dh-sequence-foo\nDescription: test\n test\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
