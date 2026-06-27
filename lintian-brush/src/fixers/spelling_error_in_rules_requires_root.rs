use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// The canonical, correctly-spelled field name.
const CANONICAL: &str = "Rules-Requires-Root";

/// Whether `field` is a misspelling of `Rules-Requires-Root` that lintian
/// flags. Mirrors lintian's regex `^ Rules? - Requires? - Roots? $` with the
/// `xi` flags (case-insensitive): the verb may be "Rule"/"Rules", "Require"/
/// "Requires" and the noun "Root"/"Roots". The exact canonical spelling is not
/// a misspelling.
fn is_misspelled_r3(field: &str) -> bool {
    if field == CANONICAL {
        return false;
    }
    let mut parts = field.split('-');
    let (Some(first), Some(second), Some(third), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    matches!(first.to_ascii_lowercase().as_str(), "rule" | "rules")
        && matches!(second.to_ascii_lowercase().as_str(), "require" | "requires")
        && matches!(third.to_ascii_lowercase().as_str(), "root" | "roots")
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
    let paragraph = source.as_deb822();

    let misspelled: Vec<(String, usize)> = paragraph
        .entries()
        .filter_map(|entry| {
            let key = entry.key()?;
            is_misspelled_r3(&key).then(|| (key, entry.line() + 1))
        })
        .collect();

    if misspelled.is_empty() {
        return Ok(Vec::new());
    }

    // Renaming a misspelled field to the canonical name would create a
    // duplicate if the canonical field already exists, or if more than one
    // field is misspelled. Report the issue in those cases but don't attempt
    // a fix.
    let has_canonical = paragraph.keys().any(|k| k == CANONICAL);
    let can_fix = !has_canonical && misspelled.len() == 1;

    let mut diagnostics = Vec::new();
    for (key, line_no) in misspelled {
        let issue = LintianIssue::source_with_info(
            "spelling-error-in-rules-requires-root",
            Visibility::Warning,
            vec![key.clone(), format!("[debian/control:{}]", line_no)],
        );

        let actions = if can_fix {
            vec![Action::Deb822(Deb822Action::RenameField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                from: key.clone(),
                to: CANONICAL.to_string(),
            })]
        } else {
            Vec::new()
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("Rename misspelled {} field to {}.", key, CANONICAL),
            format!("{} ⇒ {}", key, CANONICAL),
            actions,
        ));
    }

    Ok(diagnostics)
}

/// Describe the rename(s) as a single commit message line.
fn describe(_fixed: &[(Diagnostic, ActionPlan)], actions: &[Action]) -> String {
    let mut fields: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::RenameField { from, .. }) => Some(from.clone()),
            _ => None,
        })
        .collect();
    fields.sort();
    fields.dedup();
    format!(
        "Rename misspelled {} field to {}.",
        fields.join(", "),
        CANONICAL
    )
}

declare_detector! {
    name: "spelling-error-in-rules-requires-root",
    tags: ["spelling-error-in-rules-requires-root"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe(fixed, actions),
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
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_misspelled_singular_verb() {
        assert!(is_misspelled_r3("Rules-Require-Root"));
    }

    #[test]
    fn test_misspelled_singular_subject() {
        assert!(is_misspelled_r3("Rule-Requires-Root"));
    }

    #[test]
    fn test_misspelled_plural_root() {
        assert!(is_misspelled_r3("Rules-Requires-Roots"));
    }

    #[test]
    fn test_misspelled_wrong_case() {
        assert!(is_misspelled_r3("rules-require-root"));
    }

    #[test]
    fn test_canonical_not_misspelled() {
        assert!(!is_misspelled_r3("Rules-Requires-Root"));
    }

    #[test]
    fn test_unrelated_field_not_misspelled() {
        assert!(!is_misspelled_r3("Build-Depends"));
        assert!(!is_misspelled_r3("Rules-Requires-Root-Extra"));
        assert!(!is_misspelled_r3("Rules-Requires"));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_fix_singular_verb() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(tmp.path(), "Source: blah\nRules-Require-Root: no\n");
        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Rename misspelled Rules-Require-Root field to Rules-Requires-Root."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Source: blah\nRules-Requires-Root: no\n",
        );
    }

    #[test]
    fn test_value_preserved() {
        let tmp = TempDir::new().unwrap();
        let path = write_control(
            tmp.path(),
            "Source: blah\nRule-Requires-Root: binary-targets\n",
        );
        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Source: blah\nRules-Requires-Root: binary-targets\n",
        );
    }

    #[test]
    fn test_no_change_when_correct() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: blah\nRules-Requires-Root: no\n");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_back_off_when_multiple_misspelled() {
        // Two misspelled fields would both rename onto the same canonical
        // name, creating a duplicate; back off rather than guess.
        let tmp = TempDir::new().unwrap();
        let original = "Source: blah\nRules-Require-Root: no\nRule-Requires-Root: binary-targets\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_back_off_when_canonical_present() {
        // Both a misspelled and the canonical field are present; renaming
        // would create a duplicate, so don't touch it.
        let tmp = TempDir::new().unwrap();
        let original =
            "Source: blah\nRules-Requires-Root: no\nRules-Require-Root: binary-targets\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
