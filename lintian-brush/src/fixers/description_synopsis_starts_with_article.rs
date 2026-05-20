use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description synopsis starts with an article.";
const LABEL: &str = "Remove leading article from Description synopsis.";

/// Strip a leading indefinite or definite article from a synopsis.
///
/// Mirrors lintian's `/^(an?|the)\s/i` check: the article must be
/// "a", "an" or "the" (any case) followed by whitespace. Returns the
/// synopsis with the article and the whitespace after it removed, or
/// `None` if no article is present (or removing it would leave the
/// synopsis empty).
fn strip_article(synopsis: &str) -> Option<String> {
    let lower = synopsis.to_lowercase();
    for article in ["an", "a", "the"] {
        let Some(rest) = lower.strip_prefix(article) else {
            continue;
        };
        // The character after the article must be whitespace.
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        let stripped = synopsis[article.len()..].trim_start();
        if stripped.is_empty() {
            return None;
        }
        return Some(stripped.to_string());
    }
    None
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(description) = binary.description() else {
            continue;
        };
        // The synopsis is the first line of the Description field;
        // continuation lines describe the extended description.
        let (synopsis, rest) = match description.split_once('\n') {
            Some((s, r)) => (s, Some(r)),
            None => (description.as_str(), None),
        };
        let Some(new_synopsis) = strip_article(synopsis) else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };

        let new_description = match rest {
            Some(rest) => format!("{new_synopsis}\n{rest}"),
            None => new_synopsis,
        };

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-synopsis-starts-with-article",
            Visibility::Warning,
            vec![],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                DESCRIPTION,
                LABEL,
                vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: package_name,
                    },
                    field: "Description".into(),
                    value: new_description,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "description-synopsis-starts-with-article",
    tags: ["description-synopsis-starts-with-article"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_strip_article() {
        assert_eq!(strip_article("a tool for X").as_deref(), Some("tool for X"));
        assert_eq!(strip_article("an editor").as_deref(), Some("editor"));
        assert_eq!(strip_article("the widget").as_deref(), Some("widget"));
        assert_eq!(strip_article("A tool").as_deref(), Some("tool"));
        assert_eq!(
            strip_article("An Emacs mode").as_deref(),
            Some("Emacs mode")
        );
        assert_eq!(strip_article("THE server").as_deref(), Some("server"));
        // Multiple spaces after the article collapse away.
        assert_eq!(strip_article("a   gadget").as_deref(), Some("gadget"));
    }

    #[test]
    fn test_strip_article_no_article() {
        assert_eq!(strip_article("tool for X"), None);
        // "android" starts with "an" but is not the article "an".
        assert_eq!(strip_article("android helper"), None);
        // "apple" starts with "a" but is not the article "a".
        assert_eq!(strip_article("apple parser"), None);
        // Leading whitespace is a separate tag; leave it alone.
        assert_eq!(strip_article(" a tool"), None);
        // Article only, nothing left over.
        assert_eq!(strip_article("a "), None);
    }

    #[test]
    fn test_fix_synopsis_with_article() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: A tool for testing\n Extended description here.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: tool for testing\n Extended description here.\n",
        );
    }

    #[test]
    fn test_fix_synopsis_only() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test\nDescription: The widget library\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test\nDescription: widget library\n",
        );
    }

    #[test]
    fn test_no_article() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original =
            "Source: test\n\nPackage: test\nDescription: command-line tool\n Extended.\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_word_starting_with_article() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let original = "Source: test\n\nPackage: test\nDescription: another build tool\n";
        fs::write(debian.join("control"), original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_packages() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test\n\nPackage: test1\nDescription: A first package\n\nPackage: test2\nDescription: The second package\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test\n\nPackage: test1\nDescription: first package\n\nPackage: test2\nDescription: second package\n",
        );
    }

    #[test]
    fn test_no_description_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\n\nPackage: test\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
