use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_control::lossless::relations::Relations;
use std::path::PathBuf;

fn is_implied_by_any(
    entry: &debian_control::lossless::relations::Entry,
    stronger_relations: &[&Relations],
) -> bool {
    for relations in stronger_relations {
        for stronger_entry in relations.entries() {
            if entry.is_implied_by(&stronger_entry) {
                return true;
            }
        }
    }
    false
}

/// Walk a weaker dependency field and emit (issue, package_name_to_drop)
/// pairs for each entry that's implied by a stronger one.
fn collect_redundant<'a>(
    package: &'a str,
    field_name: &'a str,
    field_value: &str,
    stronger_relations: &[&Relations],
    pending: &mut Vec<(LintianIssue, &'a str, String)>,
) {
    if field_value.is_empty() {
        return;
    }
    let (relations, _) = Relations::parse_relaxed(field_value, true);
    for entry in relations.entries() {
        if !is_implied_by_any(&entry, stronger_relations) {
            continue;
        }
        for relation in entry.relations() {
            let Some(name) = relation.try_name() else {
                continue;
            };
            let issue = LintianIssue::binary_with_info(
                package,
                "redundant-installation-prerequisite",
                vec![format!("{} in {}", name, field_name)],
            );
            pending.push((issue, field_name, name));
        }
    }
}

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

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for binary in control.binaries() {
        let Some(package_name) = binary.name() else {
            continue;
        };
        let p = binary.as_deb822();
        let depends = p.get("Depends").unwrap_or_default();
        let pre_depends = p.get("Pre-Depends").unwrap_or_default();
        let recommends = p.get("Recommends").unwrap_or_default();
        let suggests = p.get("Suggests").unwrap_or_default();

        let (depends_rel, _) = Relations::parse_relaxed(&depends, true);
        let (pre_depends_rel, _) = Relations::parse_relaxed(&pre_depends, true);
        let (recommends_rel, _) = Relations::parse_relaxed(&recommends, true);

        let mut pending: Vec<(LintianIssue, &str, String)> = Vec::new();
        collect_redundant(
            &package_name,
            "Recommends",
            &recommends,
            &[&depends_rel, &pre_depends_rel],
            &mut pending,
        );
        collect_redundant(
            &package_name,
            "Suggests",
            &suggests,
            &[&depends_rel, &pre_depends_rel, &recommends_rel],
            &mut pending,
        );

        for (issue, field, pkg) in pending {
            diagnostics.push(Diagnostic::with_actions(
                issue,
                String::new(),
                vec![Action::Deb822(Deb822Action::DropRelation {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name.clone(),
                    },
                    field: field.to_string(),
                    package: pkg,
                })],
            ));
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if diagnostics.len() == 1 {
        "Remove redundant dependency from weaker field.".to_string()
    } else {
        format!(
            "Remove {} redundant dependencies from weaker fields.",
            diagnostics.len()
        )
    };
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "redundant-installation-prerequisite",
    tags: ["redundant-installation-prerequisite"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_from_recommends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nRecommends: foo, bar\nDescription: Test package\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nRecommends: bar\nDescription: Test package\n Test\n",
        );
    }

    #[test]
    fn test_remove_from_suggests() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nSuggests: foo, baz\nDescription: Test package\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nSuggests: baz\nDescription: Test package\n Test\n",
        );
    }

    #[test]
    fn test_remove_entire_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nRecommends: foo\nDescription: Test package\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nDescription: Test package\n Test\n",
        );
    }

    #[test]
    fn test_no_redundancy() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: mypackage\n\nPackage: mypackage\nArchitecture: any\nDepends: foo\nRecommends: bar\nSuggests: baz\nDescription: Test package\n Test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
