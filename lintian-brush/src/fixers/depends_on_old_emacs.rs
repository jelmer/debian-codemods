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

/// Obsolete emacs binary package names lintian flags. Built from
/// `{emacs21, emacs22, emacs23} x {"", -el, -gtk, -nox, -lucid}` to match
/// lintian's own `%known_obsolete_emacs` table.
static OBSOLETE_EMACS: LazyLock<std::collections::HashSet<String>> = LazyLock::new(|| {
    let mut s = std::collections::HashSet::new();
    for version in ["21", "22", "23"] {
        for flavor in ["", "-el", "-gtk", "-nox", "-lucid"] {
            s.insert(format!("emacs{}{}", version, flavor));
        }
    }
    s
});

/// For each entry in `value` whose first relation names an obsolete emacs
/// flavor, return `(old_package_name, to_entry)` where `to_entry` is the
/// literal entry text we want the applier to substitute in: the first
/// relation renamed to plain `emacs` (keeping its version constraint),
/// followed by any trailing alternatives unchanged.
fn rewrites(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        // Lintian only fires on the entry's first alternative.
        let mut rels = entry.relations();
        let Some(first) = rels.next() else { continue };
        let Some(name) = first.try_name() else {
            continue;
        };
        if !OBSOLETE_EMACS.contains(&name) {
            continue;
        }

        let first_text = match first.version() {
            Some((vc, ver)) => format!("emacs ({} {})", vc, ver),
            None => "emacs".to_string(),
        };
        let mut parts = vec![first_text];
        for r in rels {
            parts.push(r.to_string());
        }
        out.push((name, parts.join(" | ")));
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
        for field in BINARY_DEP_FIELDS {
            let Some(value) = binary.as_deb822().get(field) else {
                continue;
            };
            for (old_name, to_entry) in rewrites(&value) {
                let issue = LintianIssue {
                    package: Some(pkg_name.clone()),
                    package_type: Some(PackageType::Binary),
                    visibility: Some(Visibility::Warning),
                    tag: Some("depends-on-old-emacs".to_string()),
                    info: Some(format!("{}: {}", field, old_name)),
                };
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!(
                        "Binary package {} lists obsolete emacs flavor {} first.",
                        pkg_name, old_name
                    ),
                    format!("Replace {} with emacs in {}.", old_name, pkg_name),
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
        let (pkg, olds) = by_pkg.iter().next().unwrap();
        if olds.len() == 1 {
            let old = olds.iter().next().unwrap();
            return format!("Replace {} with emacs in {}.", old, pkg);
        }
    }
    "Replace obsolete emacs flavors with emacs.".to_string()
}

declare_detector! {
    name: "depends-on-old-emacs",
    tags: ["depends-on-old-emacs"],
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
    fn test_obsolete_set_covers_known_flavors() {
        assert!(OBSOLETE_EMACS.contains("emacs23"));
        assert!(OBSOLETE_EMACS.contains("emacs23-nox"));
        assert!(OBSOLETE_EMACS.contains("emacs21-lucid"));
        assert!(!OBSOLETE_EMACS.contains("emacs24"));
        assert!(!OBSOLETE_EMACS.contains("emacs"));
    }

    #[test]
    fn test_rewrites_simple() {
        assert_eq!(
            rewrites("emacs23"),
            vec![("emacs23".to_string(), "emacs".to_string())],
        );
    }

    #[test]
    fn test_rewrites_preserves_version() {
        assert_eq!(
            rewrites("emacs23 (>= 23.4)"),
            vec![("emacs23".to_string(), "emacs (>= 23.4)".to_string())],
        );
    }

    #[test]
    fn test_rewrites_preserves_alternatives() {
        assert_eq!(
            rewrites("emacs23 | xemacs"),
            vec![("emacs23".to_string(), "emacs | xemacs".to_string())],
        );
    }

    #[test]
    fn test_obsolete_not_first_alt_unchanged() {
        // Lintian only flags when the obsolete flavor is the first
        // alternative. xemacs | emacs23 doesn't tag, and we leave it alone.
        assert_eq!(rewrites("xemacs | emacs23"), vec![]);
    }

    #[test]
    fn test_modern_emacs_unchanged() {
        assert_eq!(rewrites("emacs"), vec![]);
        assert_eq!(rewrites("emacs (>= 28)"), vec![]);
        assert_eq!(rewrites("emacs24"), vec![]);
    }

    #[test]
    fn test_replaces_in_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: emacs23, libfoo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.description, "Replace emacs23 with emacs in foo.");
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: emacs, libfoo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_preserves_alternatives_in_replacement() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDepends: emacs23-nox | xemacs21\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: emacs | xemacs21\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_no_change_when_no_obsolete_emacs() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\n\nPackage: foo\nDepends: emacs, libfoo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(detect_in(base).unwrap(), vec![]);
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
