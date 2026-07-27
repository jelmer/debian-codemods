use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use lazy_regex::regex_is_match;
use std::path::PathBuf;

/// Source build-dependency fields lintian collects into Build-Depends-All.
const SOURCE_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// The replacement package for `name`, if it is a Python development
/// package: the same name without the `-dev` suffix.
///
/// `$PYTHON_DEV` in `Lintian::Check::Fields::PackageRelations` spells out
/// `python3-dev`, `python3-all-dev` and a fixed set of `pythonX.Y-dev`
/// versions. Matching the versioned ones by pattern rather than by an
/// enumeration keeps this working as lintian's list gains interpreter
/// versions; the extra names it accepts are all packages for which
/// dropping `-dev` is the right advice anyway.
fn replacement_for(name: &str) -> Option<&str> {
    if !regex_is_match!(r"^python(?:3|[23]\.\d+)(?:-all)?-dev$", name) {
        return None;
    }
    name.strip_suffix("-dev")
}

/// A Python dev build-dependency that can be rewritten.
#[derive(Debug, PartialEq, Eq)]
struct Rewrite {
    /// Package named in the build-dependency, e.g. `python3-dev`.
    from_package: String,
    /// Literal entry text to substitute in, e.g. `python3 (>= 3.11)`.
    to_entry: String,
}

/// Find the Python dev build-dependencies in `value` that can be rewritten
/// to their non-dev counterpart.
///
/// Entries that are part of an alternative group (`python3-dev | foo`) are
/// skipped: which branch of the alternative the maintainer wants is not
/// obvious, so back off rather than guess.
fn rewrites_for_field(value: &str) -> Vec<Rewrite> {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    let mut rewrites = Vec::new();
    for entry in relations.entries() {
        let rels: Vec<_> = entry.relations().collect();
        if rels.len() != 1 {
            continue;
        }
        let r = &rels[0];
        let Some(name) = r.try_name() else {
            continue;
        };
        let Some(plain) = replacement_for(&name) else {
            continue;
        };
        // The dev packages carry an architecture qualifier in lintian's
        // list (`python3-dev:any`); the non-dev replacement doesn't need
        // one, so any qualifier on the original is dropped.
        let to_entry = match r.version() {
            Some((vc, ver)) => format!("{} ({} {})", plain, vc, ver),
            None => plain.to_string(),
        };
        rewrites.push(Rewrite {
            from_package: name,
            to_entry,
        });
    }
    rewrites
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // lintian only fires this when the source builds no architecture-
    // dependent binary packages at all. A binary paragraph without an
    // Architecture field counts as arch-dependent, matching lintian's
    // `$arch ne 'all'` test on the (empty) value.
    let builds_arch_any = control
        .binaries()
        .any(|b| b.as_deb822().get("Architecture").as_deref() != Some("all"));
    if builds_arch_any {
        return Ok(Vec::new());
    }
    // A source paragraph with no binary paragraphs at all isn't something
    // lintian would report on either.
    if control.binaries().next().is_none() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let mut actions = Vec::new();
    for field in SOURCE_DEP_FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        for rewrite in rewrites_for_field(&value) {
            actions.push(Action::Deb822(Deb822Action::ReplaceRelation {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: (*field).to_string(),
                from_package: rewrite.from_package,
                to_entry: rewrite.to_entry,
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    // The tag carries no info field: lintian emits it as a bare hint.
    let issue = LintianIssue::source(
        "build-depends-on-python-dev-with-no-arch-any",
        Visibility::Info,
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Build-Depends on a Python development package but builds no architecture-dependent packages.",
        "Drop the -dev suffix from the Python build dependency.",
        actions,
    )])
}

declare_detector! {
    name: "build-depends-on-python-dev-with-no-arch-any",
    tags: ["build-depends-on-python-dev-with-no-arch-any"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Arch",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
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
    fn test_replacement_for() {
        assert_eq!(replacement_for("python3-dev"), Some("python3"));
        assert_eq!(replacement_for("python3-all-dev"), Some("python3-all"));
        assert_eq!(replacement_for("python3.9-dev"), Some("python3.9"));
        assert_eq!(replacement_for("python2.7-dev"), Some("python2.7"));
        // Interpreter versions newer than the ones lintian spells out.
        assert_eq!(replacement_for("python3.13-dev"), Some("python3.13"));
        assert_eq!(
            replacement_for("python3.12-all-dev"),
            Some("python3.12-all")
        );
    }

    #[test]
    fn test_replacement_for_unrelated() {
        assert_eq!(replacement_for("python3"), None);
        assert_eq!(replacement_for("python3-all"), None);
        assert_eq!(replacement_for("libpython3-dev"), None);
        assert_eq!(replacement_for("python3-dbg-dev"), None);
        assert_eq!(replacement_for("python3-numpy-dev"), None);
        assert_eq!(replacement_for("pythonfoo-dev"), None);
        assert_eq!(replacement_for(""), None);
    }

    #[test]
    fn test_rewrites_for_field_simple() {
        assert_eq!(
            rewrites_for_field("python3-dev, debhelper-compat (= 13)"),
            vec![Rewrite {
                from_package: "python3-dev".to_string(),
                to_entry: "python3".to_string(),
            }],
        );
    }

    #[test]
    fn test_rewrites_for_field_preserves_version() {
        assert_eq!(
            rewrites_for_field("python3-all-dev (>= 3.9)"),
            vec![Rewrite {
                from_package: "python3-all-dev".to_string(),
                to_entry: "python3-all (>= 3.9)".to_string(),
            }],
        );
    }

    #[test]
    fn test_rewrites_for_field_drops_arch_qualifier() {
        assert_eq!(
            rewrites_for_field("python3-dev:any"),
            vec![Rewrite {
                from_package: "python3-dev".to_string(),
                to_entry: "python3".to_string(),
            }],
        );
    }

    #[test]
    fn test_rewrites_for_field_skips_alternatives() {
        assert_eq!(rewrites_for_field("python3-dev | python3-all-dev"), vec![]);
    }

    #[test]
    fn test_rewrites_for_field_skips_unrelated() {
        assert_eq!(
            rewrites_for_field("python3, debhelper-compat (= 13)"),
            vec![]
        );
        assert_eq!(rewrites_for_field("libpython3-dev"), vec![]);
        assert_eq!(rewrites_for_field(""), vec![]);
    }

    #[test]
    fn test_replaces_in_build_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: python3-dev, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Drop the -dev suffix from the Python build dependency.",
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: python3, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_replaces_in_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends-Indep: python3-all-dev\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends-Indep: python3-all\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_no_change_when_arch_any_package_present() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Source: foo\nBuild-Depends: python3-dev\n\nPackage: foo\nArchitecture: any\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(detect_in(base).unwrap(), vec![]);
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content,
        );
    }

    #[test]
    fn test_no_change_when_one_of_several_packages_is_arch_any() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Source: foo\nBuild-Depends: python3-dev\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n\nPackage: foo-ext\nArchitecture: any\nDescription: Foo ext\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content,
        );
    }

    #[test]
    fn test_no_change_when_architecture_field_missing() {
        // An Architecture-less binary paragraph is not "all", so lintian
        // would count it as arch-dependent.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\nBuild-Depends: python3-dev\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_alternative() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Source: foo\nBuild-Depends: python3-dev | python3-all-dev\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content,
        );
    }

    #[test]
    fn test_no_change_without_python_dev() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: python3, debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_binary_packages() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(base, "Source: foo\nBuild-Depends: python3-dev\n");

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    /// An override for the tag stops the rewrite. The same tree without
    /// the override is fixed, so this really tests the override and not
    /// some other reason for backing off.
    #[test]
    fn test_honours_override() {
        let content = "Source: foo\nBuild-Depends: python3-dev\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n";

        let overridden = TempDir::new().unwrap();
        write_control(overridden.path(), content);
        fs::create_dir_all(overridden.path().join("debian/source")).unwrap();
        fs::write(
            overridden.path().join("debian/source/lintian-overrides"),
            "build-depends-on-python-dev-with-no-arch-any\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(overridden.path()),
            Err(FixerError::NoChangesAfterOverrides(_))
        ));
        assert_eq!(
            fs::read_to_string(overridden.path().join("debian/control")).unwrap(),
            content,
        );

        let plain = TempDir::new().unwrap();
        write_control(plain.path(), content);
        run_apply(plain.path()).unwrap();
        assert_eq!(
            fs::read_to_string(plain.path().join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: python3\n\nPackage: foo\nArchitecture: all\nDescription: Foo\n bar\n",
        );
    }
}
