use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const NEW_MAINTAINER: &str = "Debian Python Team <team+python@tracker.debian.org>";

const OBSOLETE_EMAILS: &[&str] = &[
    "python-modules-team@lists.alioth.debian.org",
    "python-modules-team@alioth-lists.debian.net",
    "python-apps-team@lists.alioth.debian.org",
];

/// Parse an email address from a maintainer field value
fn parse_email(maintainer_field: &str) -> Option<&str> {
    let start = maintainer_field.rfind('<')?;
    let end = maintainer_field[start..].find('>')?;
    let email = &maintainer_field[start + 1..start + end];
    if email.is_empty() {
        None
    } else {
        Some(email)
    }
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
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
    let Some(source_name) = source.as_deb822().get("Source") else {
        return Ok(Vec::new());
    };
    let Some(maintainer) = source.as_deb822().get("Maintainer") else {
        return Ok(Vec::new());
    };
    let Some(email) = parse_email(&maintainer) else {
        return Ok(Vec::new());
    };
    if !OBSOLETE_EMAILS.contains(&email) {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info("python-teams-merged", vec![email.to_string()]);

    // Use ByKey selector so the generic deb822 path keeps the
    // Maintainer field at its current position; the typed control
    // editor's `Source::set` would reorder it.
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Update maintainer email for merge of DPMT and PAPT.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::ByKey {
                field: "Source".into(),
                value: source_name,
            },
            field: "Maintainer".into(),
            value: NEW_MAINTAINER.into(),
        })],
    )])
}

declare_fixer! {
    name: "python-teams-merged",
    tags: ["python-teams-merged"],
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
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_update_obsolete_maintainer() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: foo\nMaintainer: Python Modules Packaging Team <python-modules-team@lists.alioth.debian.org>\nUploaders: Jelmer Vernooĳ <jelmer@debian.org>\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Update maintainer email for merge of DPMT and PAPT."
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: foo\nMaintainer: Debian Python Team <team+python@tracker.debian.org>\nUploaders: Jelmer Vernooĳ <jelmer@debian.org>\n",
        );
    }

    #[test]
    fn test_no_maintainer_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: foo\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_obsolete_maintainer() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nMaintainer: John Doe <john@example.com>\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_parse_email() {
        assert_eq!(
            parse_email("John Doe <john@example.com>"),
            Some("john@example.com")
        );
        assert_eq!(parse_email("John Doe"), None);
        assert_eq!(parse_email(""), None);
        assert_eq!(parse_email("<>"), None);
    }
}
