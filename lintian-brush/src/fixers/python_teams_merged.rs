use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

/// Parse an email address from a maintainer field value
fn parse_email(maintainer_field: &str) -> Option<&str> {
    // Simple email parsing - look for text between < and >
    if let Some(start) = maintainer_field.rfind('<') {
        if let Some(end) = maintainer_field[start..].find('>') {
            let email = &maintainer_field[start + 1..start + end];
            if !email.is_empty() {
                return Some(email);
            }
        }
    }
    None
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        if let Some(old_maintainer) = paragraph.get("Maintainer") {
            let old_maintainer_str = old_maintainer.to_string();

            if let Some(email) = parse_email(&old_maintainer_str) {
                let obsolete_emails = [
                    "python-modules-team@lists.alioth.debian.org",
                    "python-modules-team@alioth-lists.debian.net",
                    "python-apps-team@lists.alioth.debian.org",
                ];

                if obsolete_emails.contains(&email) {
                    let issue = LintianIssue::source_with_info(
                        "python-teams-merged",
                        vec![email.to_string()],
                    );

                    if !issue.should_fix(base_path) {
                        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                    }

                    paragraph.set(
                        "Maintainer",
                        "Debian Python Team <team+python@tracker.debian.org>",
                    );
                    editor.commit()?;

                    return Ok(FixerResult::builder(
                        "Update maintainer email for merge of DPMT and PAPT.",
                    )
                    .fixed_issue(issue)
                    .build());
                }
            }
        }
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "python-teams-merged",
    tags: ["python-teams-merged"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_update_obsolete_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: foo\nMaintainer: Python Modules Packaging Team <python-modules-team@lists.alioth.debian.org>\nUploaders: Jelmer Vernooĳ <jelmer@debian.org>\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Update maintainer email for merge of DPMT and PAPT."
        );

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content
            .contains("Maintainer: Debian Python Team <team+python@tracker.debian.org>"));
        assert!(updated_content.contains("Uploaders: Jelmer Vernooĳ <jelmer@debian.org>"));
    }

    #[test]
    fn test_no_maintainer_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: foo\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_obsolete_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: foo\nMaintainer: John Doe <john@example.com>\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_parse_email() {
        assert_eq!(
            parse_email("John Doe <john@example.com>"),
            Some("john@example.com")
        );
        assert_eq!(
            parse_email(
                "Python Modules Packaging Team <python-modules-team@lists.alioth.debian.org>"
            ),
            Some("python-modules-team@lists.alioth.debian.org")
        );
        assert_eq!(parse_email("John Doe"), None);
        assert_eq!(parse_email(""), None);
        assert_eq!(parse_email("<>"), None);
    }
}
