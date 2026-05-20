use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Substvar populated by `dh_apache2` (from `apache2-dev`); expands to the
/// `apache2-api-YYYYMMNN` virtual package the module was built against.
const APACHE2_DEPENDS: &str = "${apache2:Depends}";

/// Whether `field_value` already pins the Apache2 module API.
///
/// lintian is satisfied by an `apache2-api-*` relation in any strong
/// relation (Depends, Pre-Depends) or in Recommends. The `${apache2:Depends}`
/// substvar expands to exactly such a relation, so its presence counts too.
fn satisfies_apache2_api(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);
    if relations.substvars().any(|s| s == APACHE2_DEPENDS) {
        return true;
    }
    let has_api = relations.entries().any(|entry| {
        entry.relations().any(|rel| {
            rel.try_name()
                .map(|n| n.starts_with("apache2-api-"))
                .unwrap_or(false)
        })
    });
    has_api
}

/// Whether the relations field names a relation on `package`.
fn relation_on(field_value: &str, package: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);
    let found = relations.entries().any(|entry| {
        entry
            .relations()
            .any(|rel| rel.try_name().as_deref() == Some(package))
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Compiling an Apache2 module needs the server headers, so the source
    // build-depends on apache2-dev. Requiring it confirms the package
    // really builds a module and that dh_apache2 is present to populate
    // ${apache2:Depends}.
    let builds_against_apache2 = ["Build-Depends", "Build-Depends-Arch"]
        .iter()
        .filter_map(|f| source.as_deb822().get(f))
        .any(|v| relation_on(&v, "apache2-dev"));
    if !builds_against_apache2 {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        // Apache2 module packages follow the libapache2-mod-<foo> naming
        // scheme that lintian itself expects.
        if !package.starts_with("libapache2-mod-") {
            continue;
        }
        // A compiled .so is architecture-dependent; an Architecture: all
        // package of this name is a transitional or metapackage that ships
        // no module and so is not flagged by lintian.
        let arch = binary.as_deb822().get("Architecture");
        if matches!(arch.as_deref(), Some("all") | None) {
            continue;
        }
        let already_pinned = ["Depends", "Pre-Depends", "Recommends"]
            .iter()
            .filter_map(|f| binary.as_deb822().get(f))
            .any(|v| satisfies_apache2_api(&v));
        if already_pinned {
            continue;
        }

        let issue = LintianIssue::binary_with_info(
            &package,
            "apache2-module-does-not-depend-on-apache2-api",
            Visibility::Error,
            vec![],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "Apache2 module package {} does not depend on apache2-api.",
                package
            ),
            format!("Add {} to Depends for {}.", APACHE2_DEPENDS, package),
            vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package.clone(),
                },
                field: "Depends".into(),
                substvar: APACHE2_DEPENDS.into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut packages: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::EnsureSubstvar {
                paragraph: ParagraphSelector::Binary { package },
                ..
            }) => Some(package.as_str()),
            _ => None,
        })
        .collect();
    packages.sort();
    packages.dedup();
    format!(
        "Add {} to Depends for {}.",
        APACHE2_DEPENDS,
        packages.join(", ")
    )
}

declare_detector! {
    name: "apache2-module-does-not-depend-on-apache2-api",
    tags: ["apache2-module-does-not-depend-on-apache2-api"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Arch",
        },
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

    fn write_control(base: &Path, contents: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), contents).unwrap();
    }

    #[test]
    fn test_adds_substvar() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: debhelper-compat (= 13), apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}\nDescription: Foo module\n A module.\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Add ${apache2:Depends} to Depends for libapache2-mod-foo."
        );
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("apache2-module-does-not-depend-on-apache2-api")
        );

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: libapache2-mod-foo\nBuild-Depends: debhelper-compat (= 13), apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${shlibs:Depends}, ${misc:Depends}, ${apache2:Depends}\nDescription: Foo module\n A module.\n",
        );
    }

    #[test]
    fn test_creates_depends_field() {
        // No Depends field at all: one is created.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDescription: Foo module\n A module.\n",
        );

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${apache2:Depends}\nDescription: Foo module\n A module.\n",
        );
    }

    #[test]
    fn test_build_depends_arch() {
        // apache2-dev declared in Build-Depends-Arch is honoured.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: debhelper-compat (= 13)\nBuild-Depends-Arch: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: Foo module\n A module.\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
    }

    #[test]
    fn test_already_has_substvar() {
        let tmp = TempDir::new().unwrap();
        let original = "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${apache2:Depends}, ${misc:Depends}\nDescription: Foo module\n A module.\n";
        write_control(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_already_has_literal_api() {
        // A literal apache2-api-* relation satisfies lintian.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: apache2-api-20120211, ${misc:Depends}\nDescription: Foo module\n A module.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_api_in_recommends() {
        // lintian also accepts the API relation in Recommends.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${misc:Depends}\nRecommends: apache2-api-20120211\nDescription: Foo module\n A module.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_apache2_dev_build_dep() {
        // Without apache2-dev in the build dependencies the package is not
        // confidently an Apache2 module, and the substvar would be empty.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: Foo module\n A module.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_arch_all_transitional() {
        // An Architecture: all package ships no compiled module.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: libapache2-mod-foo\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: all\nDepends: libapache2-mod-bar\nDescription: Transitional package\n A transitional package.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_module_package() {
        // A package not named libapache2-mod-* is left alone.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: foo\nBuild-Depends: apache2-dev\n\nPackage: foo-utils\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: Utilities\n Some utilities.\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_modules() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: apache2-mods\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: Foo module\n A module.\n\nPackage: libapache2-mod-bar\nArchitecture: any\nDepends: ${misc:Depends}\nDescription: Bar module\n A module.\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        assert_eq!(
            result.description,
            "Add ${apache2:Depends} to Depends for libapache2-mod-bar, libapache2-mod-foo."
        );

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: apache2-mods\nBuild-Depends: apache2-dev\n\nPackage: libapache2-mod-foo\nArchitecture: any\nDepends: ${misc:Depends}, ${apache2:Depends}\nDescription: Foo module\n A module.\n\nPackage: libapache2-mod-bar\nArchitecture: any\nDepends: ${misc:Depends}, ${apache2:Depends}\nDescription: Bar module\n A module.\n",
        );
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
