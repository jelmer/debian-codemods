use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::relations::Relations;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep"];

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_abs = base_path.join("debian/rules");
    let rules_content = match std::fs::read(&rules_abs) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let uses_cdbs = rules_content
        .windows(b"/usr/share/cdbs/".len())
        .any(|w| w == b"/usr/share/cdbs/");
    if uses_cdbs {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    let mut emitted_issue = false;
    for field in FIELDS {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) = Relations::parse_relaxed(&value, true);
        let has_cdbs = relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("cdbs"))
        });
        if !has_cdbs {
            continue;
        }

        // Only emit one tagged diagnostic for the whole fixer (the lintian
        // tag is per source, not per field). A second matching field
        // produces an untagged diagnostic so its action still applies.
        let action = Action::Deb822(Deb822Action::DropRelation {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: (*field).to_string(),
            package: "cdbs".into(),
        });
        let message = "Drop unused build-dependency on cdbs.";
        if emitted_issue {
            diagnostics.push(Diagnostic::untagged(message, vec![action]));
        } else {
            let issue = LintianIssue::source_with_info(
                "unused-build-dependency-on-cdbs",
                vec!["[debian/rules]".to_string()],
            );
            diagnostics.push(Diagnostic::with_actions(issue, message, vec![action]));
            emitted_issue = true;
        }
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "unused-build-dependency-on-cdbs",
    tags: ["unused-build-dependency-on-cdbs"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_removes_unused_cdbs() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("rules"), "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\n",
        );
    }

    #[test]
    fn test_keeps_cdbs_when_used() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\ninclude /usr/share/cdbs/1/rules/debhelper.mk\n",
        )
        .unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
