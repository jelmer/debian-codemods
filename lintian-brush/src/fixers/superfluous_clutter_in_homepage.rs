use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Remove superfluous markup from Homepage field.";
const LABEL: &str = "Remove superfluous markup from Homepage field.";

/// Strip the surrounding `<`/`>` (and optional `URL:` / `URI:` prefix)
/// from a Homepage value, matching lintian's detection regex
/// `^<(URL:|URI:)?...>$` (case-insensitive).
fn strip_clutter(homepage: &str) -> Option<String> {
    let inner = homepage.strip_prefix('<')?.strip_suffix('>')?;
    let inner = match inner.get(..4) {
        Some(prefix) if prefix.eq_ignore_ascii_case("URL:") => &inner[4..],
        Some(prefix) if prefix.eq_ignore_ascii_case("URI:") => &inner[4..],
        _ => inner,
    };
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
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

    let Some(homepage) = source.get("Homepage") else {
        return Ok(Vec::new());
    };

    let Some(new_homepage) = strip_clutter(&homepage) else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue::source_with_info(
        "superfluous-clutter-in-homepage",
        Visibility::Warning,
        vec![homepage.clone()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        DESCRIPTION,
        LABEL,
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: new_homepage,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "superfluous-clutter-in-homepage",
    tags: ["superfluous-clutter-in-homepage"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Homepage",
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

    #[test]
    fn test_strip_clutter_plain() {
        assert_eq!(
            strip_clutter("<https://example.com/foo>").as_deref(),
            Some("https://example.com/foo"),
        );
    }

    #[test]
    fn test_strip_clutter_url_prefix() {
        assert_eq!(
            strip_clutter("<URL:https://example.com/foo>").as_deref(),
            Some("https://example.com/foo"),
        );
    }

    #[test]
    fn test_strip_clutter_uri_prefix() {
        assert_eq!(
            strip_clutter("<URI:https://example.com/foo>").as_deref(),
            Some("https://example.com/foo"),
        );
    }

    #[test]
    fn test_strip_clutter_prefix_case_insensitive() {
        assert_eq!(
            strip_clutter("<url:https://example.com/foo>").as_deref(),
            Some("https://example.com/foo"),
        );
        assert_eq!(
            strip_clutter("<uri:https://example.com/foo>").as_deref(),
            Some("https://example.com/foo"),
        );
    }

    #[test]
    fn test_strip_clutter_no_brackets() {
        assert_eq!(strip_clutter("https://example.com/foo"), None);
    }

    #[test]
    fn test_strip_clutter_only_open_bracket() {
        assert_eq!(strip_clutter("<https://example.com/foo"), None);
    }

    #[test]
    fn test_strip_clutter_only_close_bracket() {
        assert_eq!(strip_clutter("https://example.com/foo>"), None);
    }

    #[test]
    fn test_strip_clutter_empty_inside() {
        assert_eq!(strip_clutter("<>"), None);
        assert_eq!(strip_clutter("<URL:>"), None);
    }

    #[test]
    fn test_fix_removes_angle_brackets() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: <https://example.com/foo>\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://example.com/foo\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
    }

    #[test]
    fn test_fix_strips_url_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nHomepage: <URL:https://example.com/foo>\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: test-package\nHomepage: https://example.com/foo\n\nPackage: test-package\nDescription: Test\n Testing\n",
        );
    }

    #[test]
    fn test_no_change_plain_url() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let original = "Source: test-package\nHomepage: https://example.com/foo\n\nPackage: test-package\nDescription: Test\n Testing\n";
        fs::write(debian_dir.join("control"), original).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_no_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\n\nPackage: test-package\nDescription: Test\n Testing\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_diagnostic_carries_action() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nHomepage: <URL:https://example.com/foo>\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();

        let ws = FsWorkspace::new(tmp.path(), Some("foo".into()), Some("1.0".parse().unwrap()));
        let diags = detect(&ws, &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].plans[0].actions.len(), 1);
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Homepage".into(),
                value: "https://example.com/foo".into(),
            })
        );
        assert_eq!(
            diags[0].issue.as_ref().unwrap().info.as_deref(),
            Some("<URL:https://example.com/foo>")
        );
    }
}
