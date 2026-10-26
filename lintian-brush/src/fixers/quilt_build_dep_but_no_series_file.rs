use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const DESCRIPTION: &str = "quilt is in Build-Depends but there is no debian/patches/series file.";

const BUILD_DEP_FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// True when the relations field has a standalone `quilt` entry (no
/// alternatives), which is what lintian's `satisfies('quilt')` checks for.
fn has_standalone_quilt(value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(value, true);
    let found = relations.entries().any(|entry| {
        let rels: Vec<_> = entry.relations().collect();
        rels.len() == 1 && rels[0].try_name().as_deref() == Some("quilt")
    });
    found
}

/// Whether debian/rules may rely on quilt being installed, either by
/// mentioning it or by including makefiles from the source tree that we
/// don't inspect.
fn rules_may_use_quilt(ws: &dyn Workspace) -> Result<bool, FixerError> {
    let Some(text) = ws.read_file(Path::new("debian/rules"))? else {
        return Ok(false);
    };
    if String::from_utf8_lossy(&text)
        .to_ascii_lowercase()
        .contains("quilt")
    {
        return Ok(true);
    }
    let makefile = ws.parsed_rules()?;
    let has_local_include = makefile
        .included_files()
        .any(|path| !path.starts_with("/usr/share/"));
    Ok(has_local_include)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // 3.0 (quilt) sources do not need quilt; lintian doesn't check those.
    if ws.source_format()?.as_deref() == Some("3.0 (quilt)") {
        return Ok(Vec::new());
    }

    if ws.read_file(Path::new("debian/patches/series"))?.is_some() {
        return Ok(Vec::new());
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let fields: Vec<&str> = BUILD_DEP_FIELDS
        .iter()
        .copied()
        .filter(|field| {
            source
                .as_deb822()
                .get(field)
                .is_some_and(|value| has_standalone_quilt(&value))
        })
        .collect();
    if fields.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "quilt-build-dep-but-no-series-file",
        Visibility::Warning,
        vec![],
    );

    // Any patches present without a series file means we can't tell how
    // they're meant to be applied, so leave it to the maintainer.
    let has_patches = ws
        .list_dir(Path::new("debian/patches"))?
        .is_some_and(|entries| !entries.is_empty());
    if has_patches || rules_may_use_quilt(ws)? {
        return Ok(vec![Diagnostic::with_plans(issue, DESCRIPTION, Vec::new())]);
    }

    let control_rel = PathBuf::from("debian/control");
    let actions = fields
        .into_iter()
        .map(|field| {
            Action::Deb822(Deb822Action::DropRelation {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: field.to_string(),
                package: "quilt".into(),
            })
        })
        .collect();

    Ok(vec![Diagnostic::with_actions(
        issue,
        DESCRIPTION,
        "Drop unused build-dependency on quilt.",
        actions,
    )
    .with_certainty(Certainty::Likely)])
}

declare_detector! {
    name: "quilt-build-dep-but-no-series-file",
    tags: ["quilt-build-dep-but-no-series-file"],
    triggers: [
        debian_workspace::Trigger::File("debian/source/format"),
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Glob("debian/patches/*"),
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
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("blah".into()),
            Some(version),
        );
        DetectorImpl.apply(&ws, &FixerPreferences::default())
    }

    fn setup(control: &str, rules: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), control).unwrap();
        fs::write(debian.join("rules"), rules).unwrap();
        tmp
    }

    const RULES: &str = "#!/usr/bin/make -f\n\n%:\n\tdh $@\n";

    #[test]
    fn test_has_standalone_quilt() {
        assert!(has_standalone_quilt("debhelper, quilt"));
        assert!(has_standalone_quilt("quilt (>= 0.40)"));
        assert!(!has_standalone_quilt("debhelper"));
        assert!(!has_standalone_quilt("quilt | dpatch"));
        assert!(!has_standalone_quilt("quilt-el"));
    }

    #[test]
    fn test_drop_quilt() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_held_back_when_confident_required() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("blah".into()),
            Some(version),
        );
        let prefs = FixerPreferences {
            minimum_certainty: Some(Certainty::Confident),
            ..FixerPreferences::default()
        };
        assert!(matches!(
            DetectorImpl.apply(&ws, &prefs),
            Err(FixerError::NotCertainEnough(..))
        ));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_drop_quilt_from_indep() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\nBuild-Depends-Indep: quilt (>= 0.40), python3\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\nBuild-Depends-Indep: python3\n\nPackage: blah\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_series_exists() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "# no patches\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_quilt_source_format() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        fs::create_dir_all(tmp.path().join("debian/source")).unwrap();
        fs::write(tmp.path().join("debian/source/format"), "3.0 (quilt)\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_rules_uses_quilt() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with quilt\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_rules_includes_local_makefile() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            "#!/usr/bin/make -f\n\ninclude /usr/share/dpkg/default.mk\ninclude debian/patch.mk\n\n%:\n\tdh $@\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_patches_without_series() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("01-fix.patch"), "--- a\n+++ b\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_quilt_alternative() {
        let tmp = setup(
            "Source: blah\nBuild-Depends: debhelper-compat (= 13), quilt | dpatch\n\nPackage: blah\nDescription: blah\n blah\n",
            RULES,
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
