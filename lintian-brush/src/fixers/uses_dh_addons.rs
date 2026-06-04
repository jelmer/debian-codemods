use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, MakefileAction, ParagraphSelector};
use crate::{FixerError, FixerPreferences};
use debian_analyzer::rules::{dh_invoke_drop_with, dh_invoke_get_with};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// A dh addon that can be expressed as a `dh-sequence-*` build dependency.
struct Convertible {
    /// The `--with` addon name, as it appears in debian/rules.
    addon: &'static str,
    /// The `dh-sequence-*` virtual package providing the addon.
    sequence: &'static str,
    /// Build dependencies that become redundant once the package
    /// build-depends on `sequence` (they provide the same sequence).
    obsolete: &'static [&'static str],
}

/// Addons known to be provided by a `dh-sequence-*` virtual package, along
/// with the build dependency that becomes redundant after the conversion.
///
/// Restricted to cases where the providing package is a distinct add-on
/// package (not debhelper itself) and dropping it is unambiguously correct.
/// Addons whose sequence is provided by `debhelper`/`debhelper-compat`
/// (e.g. dwz, systemd) are handled by other fixers and deliberately omitted.
const CONVERTIBLE: &[Convertible] = &[
    Convertible {
        addon: "gir",
        sequence: "dh-sequence-gir",
        obsolete: &["gobject-introspection", "gobject-introspection-bin"],
    },
    Convertible {
        addon: "gnome",
        sequence: "dh-sequence-gnome",
        obsolete: &["gnome-pkg-tools"],
    },
    Convertible {
        addon: "cli",
        sequence: "dh-sequence-cli",
        obsolete: &["cli-common-dev"],
    },
    Convertible {
        addon: "sphinxdoc",
        sequence: "dh-sequence-sphinxdoc",
        obsolete: &["sphinx-common"],
    },
    Convertible {
        addon: "vim_addon",
        sequence: "dh-sequence-vim-addon",
        obsolete: &["dh-vim-addon"],
    },
    Convertible {
        addon: "perl_dbi",
        sequence: "dh-sequence-perl-dbi",
        obsolete: &["libdbi-perl"],
    },
    Convertible {
        addon: "perl_imager",
        sequence: "dh-sequence-perl-imager",
        obsolete: &["libimager-perl"],
    },
    Convertible {
        addon: "scour",
        sequence: "dh-sequence-scour",
        obsolete: &["scour"],
    },
];

fn lookup(addon: &str) -> Option<&'static Convertible> {
    CONVERTIBLE.iter().find(|c| c.addon == addon)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // The dh-sequence-* Build-Depends mechanism is keyed on the debhelper
    // version at build time (introduced in debhelper 11.4), not on the
    // declared compat level, so there is no compat floor to check: every
    // debhelper able to build the package honours it.
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    // Build dependencies currently declared, so we only drop ones present.
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let existing_bd: String = control
        .source()
        .and_then(|s| s.as_deb822().get("Build-Depends"))
        .unwrap_or_default();

    let control_rel = PathBuf::from("debian/control");
    let rules_rel = PathBuf::from("debian/rules");

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut converted: Vec<String> = Vec::new();

    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let addons = dh_invoke_get_with(&recipe);
            if addons.is_empty() {
                continue;
            }

            let mut modified = recipe.to_string();
            let mut actions: Vec<Action> = Vec::new();
            let mut local_converted: Vec<&'static str> = Vec::new();

            for addon in &addons {
                let Some(conv) = lookup(addon) else {
                    continue;
                };
                let dropped = dh_invoke_drop_with(&modified, addon);
                if dropped == modified {
                    continue;
                }
                modified = dropped;
                local_converted.push(conv.addon);

                actions.push(Action::Deb822(Deb822Action::EnsureRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: "Build-Depends".into(),
                    entry: conv.sequence.into(),
                }));
                for obsolete in conv.obsolete {
                    if relation_present(&existing_bd, obsolete) {
                        actions.push(Action::Deb822(Deb822Action::DropRelation {
                            file: control_rel.clone(),
                            paragraph: ParagraphSelector::Source,
                            field: "Build-Depends".into(),
                            package: (*obsolete).into(),
                        }));
                    }
                }
            }

            if local_converted.is_empty() {
                continue;
            }

            actions.insert(
                0,
                Action::Makefile(MakefileAction::ReplaceRecipe {
                    file: rules_rel.clone(),
                    target: target.clone(),
                    recipe: recipe.to_string(),
                    new_recipe: modified,
                }),
            );

            for c in &local_converted {
                if !converted.contains(&c.to_string()) {
                    converted.push(c.to_string());
                }
            }

            diagnostics.push(Diagnostic::untagged(
                "debian/rules uses dh addons that have dh-sequence-* packages.",
                String::new(),
                actions,
            ));
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let label = format!(
        "Use dh-sequence-* build dependencies instead of dh --with: {}.",
        converted.join(", ")
    );
    for d in &mut diagnostics {
        for plan in &mut d.plans {
            plan.label = label.clone();
            // Both forms are valid; converting is a stylistic preference that
            // also rewrites build dependencies, so only apply it on request.
            plan.opinionated = true;
        }
    }
    Ok(diagnostics)
}

/// Whether a relations field names `package` (ignoring version constraints
/// and alternatives).
fn relation_present(field: &str, package: &str) -> bool {
    use debian_control::lossless::Relations;
    let (relations, _) = Relations::parse_relaxed(field, true);
    let mut entries = relations.entries();
    entries.any(|entry| {
        entry
            .relations()
            .any(|r| r.try_name().as_deref() == Some(package))
    })
}

declare_detector! {
    name: "uses-dh-addons",
    tags: [],
    triggers: [
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
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

    fn run_apply_with(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(v.clone()),
        );
        let preferences = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        adapter.apply(&ws, &preferences)
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        run_apply_with(base, true)
    }

    fn write_package(base: &Path, control: &str, rules: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), control).unwrap();
        fs::write(debian.join("rules"), rules).unwrap();
    }

    #[test]
    fn test_converts_gir_and_drops_obsolete() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), gobject-introspection\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gir\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/rules")).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), dh-sequence-gir\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_converts_without_obsolete_build_dep() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gnome\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/rules")).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), dh-sequence-gnome\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_keeps_unknown_addon() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), gobject-introspection\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gir,quilt\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/rules")).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with quilt\n",
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), dh-sequence-gir\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_converts_two_addons_in_one_invocation() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), gobject-introspection, gnome-pkg-tools\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gir,gnome\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/rules")).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), dh-sequence-gir, dh-sequence-gnome\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_converts_at_low_compat_level() {
        // The dh-sequence-* mechanism is keyed on the debhelper version, not
        // the compat level, so the conversion applies even at compat 10.
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 10), gobject-introspection\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gir\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/rules")).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 10), dh-sequence-gir\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_change_without_addons() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_non_convertible_addon() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with quilt\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_unless_opinionated() {
        let tmp = TempDir::new().unwrap();
        write_package(
            tmp.path(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), gobject-introspection\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with gir\n",
        );
        assert!(matches!(
            run_apply_with(tmp.path(), false),
            Err(FixerError::NoChanges)
        ));
    }
}
