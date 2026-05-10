use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_control::lossless::relations::Relations;
use std::path::{Path, PathBuf};

const FIELDS: &[&str] = &["Build-Depends", "Build-Depends-Indep"];

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_content = match ws.read_file(Path::new("debian/rules"))? {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    let uses_cdbs = rules_content
        .windows(b"/usr/share/cdbs/".len())
        .any(|w| w == b"/usr/share/cdbs/");
    if uses_cdbs {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
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
        let description = "Build-Depends on cdbs but cdbs is not used.";
        let label = "Drop unused build-dependency on cdbs.";
        if emitted_issue {
            diagnostics.push(Diagnostic::untagged(description, label, vec![action]));
        } else {
            let issue = LintianIssue::source_with_info(
                "unused-build-dependency-on-cdbs",
                Visibility::Warning,
                vec!["[debian/rules]".to_string()],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                description,
                label,
                vec![action],
            ));
            emitted_issue = true;
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "unused-build-dependency-on-cdbs",
    tags: ["unused-build-dependency-on-cdbs"],
    triggers: [
        debian_workspace::Trigger::File("debian/rules"),
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
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
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
