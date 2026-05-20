use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Uploaders field lists a contact more than once.";
const LABEL: &str = "Remove duplicate entries from the Uploaders field.";

/// Extract the contact's e-mail address from a single `Uploaders` entry.
///
/// Uses [`debian_changelog::parseaddr`] — the address parser shared with
/// the rest of the codebase — and keeps only entries that parse to a
/// plausible `user@host` address. lintian's `duplicate-contact` check
/// likewise compares uploaders by parsed address and ignores entries it
/// cannot parse as a valid address.
fn contact_email(entry: &str) -> Option<&str> {
    let (_, email) = debian_changelog::parseaddr(entry.trim());
    match email.split_once('@') {
        Some((user, host)) if !user.is_empty() && !host.is_empty() => Some(email),
        _ => None,
    }
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
    let Some(uploaders) = source.get("Uploaders") else {
        return Ok(Vec::new());
    };

    // The first occurrence of each contact is kept; later occurrences
    // with an e-mail address already seen are dropped.
    let mut seen: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    for entry in uploaders
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match contact_email(entry) {
            Some(email) => {
                if seen.iter().any(|e| e == email) {
                    if !duplicates.iter().any(|d| d == email) {
                        duplicates.push(email.to_string());
                    }
                } else {
                    seen.push(email.to_string());
                    kept.push(entry.to_string());
                }
            }
            None => kept.push(entry.to_string()),
        }
    }

    if duplicates.is_empty() {
        return Ok(Vec::new());
    }

    let action = Action::Deb822(Deb822Action::SetField {
        file: PathBuf::from("debian/control"),
        paragraph: ParagraphSelector::Source,
        field: "Uploaders".into(),
        value: kept.join(", "),
    });

    // lintian emits one `duplicate-contact` hint per duplicated address;
    // a single field rewrite resolves all of them.
    Ok(duplicates
        .into_iter()
        .map(|email| {
            let issue = LintianIssue::source_with_info(
                "duplicate-contact",
                Visibility::Warning,
                vec!["Uploaders".to_string(), email],
            );
            Diagnostic::with_actions(issue, DESCRIPTION, LABEL, vec![action.clone()])
        })
        .collect())
}

declare_detector! {
    name: "duplicate-contact",
    tags: ["duplicate-contact"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Uploaders",
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

    fn write_control(base: &Path, contents: &str) {
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), contents).unwrap();
    }

    #[test]
    fn test_contact_email() {
        assert_eq!(
            contact_email("John Doe <john@example.com>"),
            Some("john@example.com")
        );
        // Bare address without a display name.
        assert_eq!(contact_email("john@example.com"), Some("john@example.com"));
        // No address at all.
        assert_eq!(contact_email("Debian QA Group"), None);
        // Empty angle brackets.
        assert_eq!(contact_email("Nobody <>"), None);
    }

    #[test]
    fn test_duplicate_uploader() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nUploaders: Jane <jane@example.com>, Jane <jane@example.com>, Joe <joe@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nUploaders: Jane <jane@example.com>, Joe <joe@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_duplicate_by_email_different_name() {
        // Same address, different display name: still a duplicate.
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nUploaders: Jane Doe <jane@example.com>, Jane <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nUploaders: Jane Doe <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_triple_duplicate() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nUploaders: Joe <joe@example.com>, Joe <joe@example.com>, Joe <joe@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        // A repeated address is reported once, not once per occurrence.
        assert_eq!(result.fixed_lintian_issues.len(), 1);

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nUploaders: Joe <joe@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_two_distinct_duplicates() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nUploaders: A <a@example.com>, B <b@example.com>, A <a@example.com>, B <b@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nUploaders: A <a@example.com>, B <b@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_bare_email() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nUploaders: joe@example.com, joe@example.com\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nUploaders: joe@example.com\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_no_duplicates() {
        let tmp = TempDir::new().unwrap();
        let original = "Source: test\nUploaders: Jane <jane@example.com>, Joe <joe@example.com>\n\nPackage: test\nDescription: Test\n Test package\n";
        write_control(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            original
        );
    }

    #[test]
    fn test_no_uploaders_field() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nMaintainer: Jane <jane@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
