use crate::diagnostic::{Action, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, LintianIssue};
use regex::Regex;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright_abs = base_path.join(&copyright_rel);
    if !copyright_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&copyright_abs)?;

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

    let issue = LintianIssue::source_with_info("old-fsf-address-in-copyright-file", vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Update FSF postal address.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_fixer! {
    name: "old-fsf-address-in-copyright-file",
    tags: ["old-fsf-address-in-copyright-file"],
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
