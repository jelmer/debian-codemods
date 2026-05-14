use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Binary dependency fields lintian's check considers; matches its
/// `is_dep_field` for the per-binary loop.
const BINARY_DEP_FIELDS: &[&str] = &["Depends", "Pre-Depends", "Recommends", "Suggests"];

static PYTHON_MINIMAL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(python[\d.]*)-minimal$").expect("static regex compiles")
});

/// If `package` is `python-minimal`, `python3-minimal`, `python3.11-minimal`,
/// etc., return the corresponding non-minimal package name (`python`,
/// `python3`, `python3.11`). Returns `None` otherwise.
fn strip_minimal(package: &str) -> Option<&str> {
    PYTHON_MINIMAL
        .captures(package)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// Walk `value`, looking for relations whose name matches `python*-minimal`.
/// Return `(old_name, to_entry)` pairs, where `to_entry` is the literal
/// replacement entry text (just the new package name plus any version
/// constraint the original carried). The applier's ReplaceRelation logic
/// handles the actual rewrite, including deduplicating against an
/// alternative whose name now matches.
fn rewrites(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        for r in entry.relations() {
            let Some(name) = r.try_name() else { continue };
            let Some(new_name) = strip_minimal(&name) else {
                continue;
            };
            let to_entry = match r.version() {
                Some((vc, ver)) => format!("{} ({} {})", new_name, vc, ver),
                None => new_name.to_string(),
            };
            out.push((name, to_entry));
        }
    }
    out
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for binary in control.binaries() {
        let Some(pkg_name) = binary.name() else {
            continue;
        };
        // The lintian check exempts python*-minimal binary packages so they
        // can keep their declared relationships on each other.
        if PYTHON_MINIMAL.is_match(&pkg_name) {
            continue;
        }
        for field in BINARY_DEP_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            for (old_name, to_entry) in rewrites(&value) {
                let issue = LintianIssue {
                    package: Some(pkg_name.clone()),
                    package_type: Some(PackageType::Binary),
                    visibility: Some(Visibility::Error),
                    tag: Some("depends-on-python-minimal".to_string()),
                    info: Some((*field).to_string()),
                };
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!("Binary package {} depends on {}.", pkg_name, old_name),
                    format!(
                        "Replace {} with {} in {}.",
                        old_name,
                        strip_minimal(&old_name).unwrap_or(""),
                        pkg_name
                    ),
                    vec![Action::Deb822(Deb822Action::ReplaceRelation {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: pkg_name.clone(),
                        },
                        field: (*field).to_string(),
                        from_package: old_name,
                        to_entry,
                    })],
                ));
            }
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut by_pkg: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> =
        std::collections::BTreeMap::new();
    for action in actions {
        if let Action::Deb822(Deb822Action::ReplaceRelation {
            paragraph: ParagraphSelector::Binary { package },
            from_package,
            ..
        }) = action
        {
            by_pkg
                .entry(package.as_str())
                .or_default()
                .insert(from_package.as_str());
        }
    }
    if by_pkg.len() == 1 {
        let (pkg, deps) = by_pkg.iter().next().unwrap();
        if deps.len() == 1 {
            let dep = deps.iter().next().unwrap();
            return format!(
                "Replace {} with {} in {}.",
                dep,
                strip_minimal(dep).unwrap_or(""),
                pkg
            );
        }
    }
    "Replace python*-minimal dependencies with non-minimal equivalents.".to_string()
}

declare_detector! {
    name: "depends-on-python-minimal",
    tags: ["depends-on-python-minimal"],
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
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_strip_minimal_python3() {
        assert_eq!(strip_minimal("python3-minimal"), Some("python3"));
        assert_eq!(strip_minimal("python-minimal"), Some("python"));
        assert_eq!(strip_minimal("python3.11-minimal"), Some("python3.11"));
    }

    #[test]
    fn test_strip_minimal_unrelated() {
        assert_eq!(strip_minimal("python3"), None);
        assert_eq!(strip_minimal("python3-dev"), None);
        assert_eq!(strip_minimal("minimal"), None);
        assert_eq!(strip_minimal("python-minimal-extras"), None);
    }

    #[test]
    fn test_rewrites_simple() {
        assert_eq!(
            rewrites("python3-minimal"),
            vec![("python3-minimal".to_string(), "python3".to_string())],
        );
    }

    #[test]
    fn test_rewrites_preserves_version() {
        assert_eq!(
            rewrites("python3-minimal (>= 3.5)"),
            vec![(
                "python3-minimal".to_string(),
                "python3 (>= 3.5)".to_string(),
            )],
        );
    }

    #[test]
    fn test_rewrites_finds_minimal_in_alternative() {
        // The to_entry text is just the renamed relation. The applier
        // handles dedup against the existing python3 alternative.
        assert_eq!(
            rewrites("python3-minimal | python3"),
            vec![("python3-minimal".to_string(), "python3".to_string())],
        );
    }

    #[test]
    fn test_rewrites_skips_non_minimal() {
        assert_eq!(rewrites("python3, libfoo"), vec![]);
        assert_eq!(rewrites(""), vec![]);
    }

    #[test]
    fn test_replaces_in_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: python3-minimal, libfoo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Replace python3-minimal with python3 in foo."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: python3, libfoo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_preserves_version_in_replacement() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: python3-minimal (>= 3.5)\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: python3 (>= 3.5)\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_dedupes_against_existing_alternative() {
        // python3-minimal | python3 becomes just python3: the applier
        // drops the from_package entry instead of inserting a duplicate.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: python3-minimal | python3\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: python3\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_skips_python_minimal_binary_itself() {
        // Lintian exempts python*-minimal binaries from this check so that
        // they can declare relationships on each other.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: python3-defaults\n\nPackage: python3-minimal\nDepends: python3.11-minimal\nDescription: Min\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(detect_in(base).unwrap(), vec![]);
    }

    #[test]
    fn test_no_change_when_no_minimal() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\n\nPackage: foo\nDepends: python3, libfoo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
