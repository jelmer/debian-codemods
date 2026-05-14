use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Source build-dependency fields lintian collects into Build-Depends-All for
/// this check.
const SOURCE_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// If `value` contains a standalone `python-sphinx` entry (no alternatives),
/// return the literal `to_entry` text the applier should substitute in (the
/// renamed package plus any version constraint that was on it). Alternatives
/// like `python-sphinx | python3-sphinx` are out of scope: they're caught by
/// the separate `alternatively-build-depends-on-python-sphinx-and-python3-sphinx`
/// tag, which has its own fix.
fn rewrite_field(value: &str) -> Option<String> {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    for entry in relations.entries() {
        let rels: Vec<_> = entry.relations().collect();
        if rels.len() != 1 {
            continue;
        }
        let r = &rels[0];
        if r.try_name().as_deref() != Some("python-sphinx") {
            continue;
        }
        return Some(match r.version() {
            Some((vc, ver)) => format!("python3-sphinx ({} {})", vc, ver),
            None => "python3-sphinx".to_string(),
        });
    }
    None
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

    // Lintian's check fires when Build-Depends-All satisfies python-sphinx
    // but not python3-sphinx. Mirror that: if python3-sphinx is already
    // anywhere in the source's build relations, there's nothing to do.
    let already_has_python3 = SOURCE_DEP_FIELDS.iter().any(|field| {
        let Some(value) = source.as_deb822().get(field) else {
            return false;
        };
        let (relations, _errors) = Relations::parse_relaxed(&value, true);
        relations.has_relation("python3-sphinx")
    });
    if already_has_python3 {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for field in SOURCE_DEP_FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let Some(to_entry) = rewrite_field(&value) else {
            continue;
        };
        let issue = LintianIssue::source_with_info(
            "build-depends-on-python-sphinx-only",
            Visibility::Warning,
            vec![(*field).to_string()],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Build-Depends on python-sphinx only.",
            "Replace python-sphinx with python3-sphinx in Build-Depends.",
            vec![Action::Deb822(Deb822Action::ReplaceRelation {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: (*field).to_string(),
                from_package: "python-sphinx".to_string(),
                to_entry,
            })],
        ));
        // One rewrite per package is enough. The lintian tag carries no
        // field info, just the fact that the source build-depends on the
        // wrong sphinx, so don't fire it multiple times if python-sphinx
        // happens to appear in several build-deps fields.
        break;
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "build-depends-on-python-sphinx-only",
    tags: ["build-depends-on-python-sphinx-only"],
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
    fn test_rewrite_field_simple() {
        assert_eq!(
            rewrite_field("python-sphinx"),
            Some("python3-sphinx".to_string()),
        );
    }

    #[test]
    fn test_rewrite_field_preserves_version() {
        assert_eq!(
            rewrite_field("python-sphinx (>= 1.4)"),
            Some("python3-sphinx (>= 1.4)".to_string()),
        );
    }

    #[test]
    fn test_rewrite_field_skips_alternative() {
        // The companion tag (alternatively-build-depends-on-python-sphinx-
        // and-python3-sphinx) handles the alt case. We deliberately leave
        // it alone here.
        assert_eq!(rewrite_field("python-sphinx | python3-sphinx"), None);
    }

    #[test]
    fn test_rewrite_field_skips_unrelated() {
        assert_eq!(rewrite_field("python3-sphinx"), None);
        assert_eq!(rewrite_field("libfoo, debhelper-compat (= 13)"), None);
        assert_eq!(rewrite_field(""), None);
    }

    #[test]
    fn test_replaces_in_build_depends() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends: python-sphinx, debhelper-compat (= 13)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Replace python-sphinx with python3-sphinx in Build-Depends.",
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends: python3-sphinx, debhelper-compat (= 13)\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_replaces_in_build_depends_indep() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nBuild-Depends-Indep: python-sphinx\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nBuild-Depends-Indep: python3-sphinx\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_no_change_when_python3_sphinx_already_present() {
        // If python3-sphinx already appears anywhere in the source's build
        // relations, lintian doesn't emit the tag.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\nBuild-Depends: python-sphinx, python3-sphinx\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(detect_in(base).unwrap(), vec![]);
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content,
        );
    }

    #[test]
    fn test_no_change_when_only_python3_sphinx() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\nBuild-Depends: python3-sphinx\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_sphinx() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_alternative_form_left_alone() {
        // python-sphinx | python3-sphinx is the companion tag's territory.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "Source: foo\nBuild-Depends: python-sphinx | python3-sphinx\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            content,
        );
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
