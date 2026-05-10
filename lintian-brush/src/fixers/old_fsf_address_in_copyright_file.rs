use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use debian_workspace::Workspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use regex::Regex;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };

    // Match the old FSF address with its leading indentation captured so
    // we can preserve it on the rewritten lines.
    let re = Regex::new(
        r"(?s)([ ]+)Free Software Foundation, Inc\., 59 Temple Place - Suite 330,\s*\n([ ]+)Boston, MA 02111-1307, USA\.",
    )
    .unwrap();

    let mut actions: Vec<Action> = Vec::new();
    for caps in re.captures_iter(&content) {
        let m = caps.get(0).unwrap();
        let replacement = format!(
            "{}Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n{}MA 02110-1301, USA.",
            &caps[1], &caps[2]
        );
        actions.push(Action::Filesystem(FilesystemAction::ReplaceText {
            file: copyright_rel.clone(),
            range: TextRange {
                start: m.start(),
                end: m.end(),
            },
            replacement,
        }));
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "old-fsf-address-in-copyright-file",
        Visibility::Warning,
        vec![],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Old FSF postal address in debian/copyright.",
        "Update FSF postal address.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "old-fsf-address-in-copyright-file",
    tags: ["old-fsf-address-in-copyright-file"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "License",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "License",
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
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_update_fsf_address() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(&copyright, "This program is free software...\n  Free Software Foundation, Inc., 59 Temple Place - Suite 330,\n  Boston, MA 02111-1307, USA.\nOn Debian systems...").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Update FSF postal address.");
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "This program is free software...\n  Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n  MA 02110-1301, USA.\nOn Debian systems...",
        );
    }

    #[test]
    fn test_no_old_fsf_address() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This program is free software...\n  Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n  MA 02110-1301, USA.\nOn Debian systems...",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_different_whitespace() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(&copyright, "License text:\n    Free Software Foundation, Inc., 59 Temple Place - Suite 330,\n    Boston, MA 02111-1307, USA.\nMore text.").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "License text:\n    Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n    MA 02110-1301, USA.\nMore text.",
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
