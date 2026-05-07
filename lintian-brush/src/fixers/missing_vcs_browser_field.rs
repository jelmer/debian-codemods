use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let para = source.as_deb822();
    if para.contains_key("Vcs-Browser") {
        return Ok(Vec::new());
    }
    let Some(vcs_git) = para.get("Vcs-Git") else {
        return Ok(Vec::new());
    };

    let Some(browser_url) =
        debian_analyzer::vcs::determine_browser_url("git", &vcs_git, preferences.net_access)
    else {
        return Ok(Vec::new());
    };

    let Some(source_name) = para.get("Source") else {
        return Ok(Vec::new());
    };
    let issue = LintianIssue::source_with_info(
        "missing-vcs-browser-field",
        vec![format!("Vcs-Git {}", vcs_git)],
    );
    // Use the generic deb822 path so the new field lands at end of the
    // source paragraph, right after Vcs-Git, instead of being shuffled
    // into the typed control editor's canonical order.
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Vcs-Browser field is missing.",
        "debian/control: Add Vcs-Browser field",
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::ByKey {
                field: "Source".into(),
                value: source_name,
            },
            field: "Vcs-Browser".into(),
            value: browser_url.to_string(),
        })],
    )])
}

declare_detector! {
    name: "missing-vcs-browser-field",
    tags: ["missing-vcs-browser-field"],
    after: ["vcs-field-uses-not-recommended-uri-format"],
    triggers: [
        // Reads any Vcs-* field on the source paragraph to derive a
        // browser URL, and writes Vcs-Browser there.
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, preferences)
    }

    #[test]
    fn test_add_vcs_browser_for_github() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nVcs-Git: git://github.com/user/repo\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &preferences).unwrap();
        assert_eq!(result.description, "debian/control: Add Vcs-Browser field");

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nVcs-Git: git://github.com/user/repo\nVcs-Browser: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_no_change_when_vcs_browser_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: git://github.com/user/repo\nVcs-Browser: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_when_no_vcs_git() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &preferences),
            Err(FixerError::NoChanges)
        ));
    }
}
