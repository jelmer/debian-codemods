use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

/// Old common-licenses paths that lintian flags, in priority order: the
/// longer `usr/share/doc/copyright` is tried before the shorter `usr/doc`
/// form so we don't half-rewrite the longer one. Both are replaced with the
/// modern `usr/share/common-licenses` location (Policy 12.5).
const OLD_DIRS: &[&str] = &["usr/share/doc/copyright", "usr/doc/copyright"];
const NEW_DIR: &str = "usr/share/common-licenses";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let Some(bytes) = ws.read_file(&copyright_rel)? else {
        return Ok(Vec::new());
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };

    let mut actions: Vec<Action> = Vec::new();
    for old in OLD_DIRS {
        if content.contains(old) {
            actions.push(Action::Filesystem(FilesystemAction::Substitute {
                file: copyright_rel.clone(),
                from: (*old).to_string(),
                to: NEW_DIR.to_string(),
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source("copyright-refers-to-old-directory", Visibility::Error);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/copyright references old common-licenses directory.",
        "Update common-licenses path to /usr/share/common-licenses.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "copyright-refers-to-old-directory",
    tags: ["copyright-refers-to-old-directory"],
    triggers: [crate::workspace::Trigger::File("debian/copyright")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{DetectorAdapter, TreeFixerWorkspace};
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = TreeFixerWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    fn write_copyright(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("copyright"), content).unwrap();
    }

    #[test]
    fn test_rewrites_usr_share_doc_copyright() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_copyright(base, "See /usr/share/doc/copyright/GPL for details.\n");

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Update common-licenses path to /usr/share/common-licenses."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::read_to_string(base.join("debian/copyright")).unwrap(),
            "See /usr/share/common-licenses/GPL for details.\n",
        );
    }

    #[test]
    fn test_rewrites_usr_doc_copyright() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_copyright(base, "See /usr/doc/copyright/GPL for details.\n");

        let result = run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/copyright")).unwrap(),
            "See /usr/share/common-licenses/GPL for details.\n",
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("copyright-refers-to-old-directory"),
        );
    }

    #[test]
    fn test_rewrites_both_old_paths_in_one_pass() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_copyright(
            base,
            "See /usr/share/doc/copyright/GPL and /usr/doc/copyright/BSD.\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/copyright")).unwrap(),
            "See /usr/share/common-licenses/GPL and /usr/share/common-licenses/BSD.\n",
        );
    }

    #[test]
    fn test_no_change_when_modern_path() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "See /usr/share/common-licenses/GPL for details.\n";
        write_copyright(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(base.join("debian/copyright")).unwrap(),
            content
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_does_not_rewrite_unrelated_usr_share_doc_paths() {
        // /usr/share/doc/<pkg>/ is a legitimate documentation path; only
        // the literal "/usr/share/doc/copyright" prefix (the historical
        // common-licenses location) gets rewritten.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "Upstream docs live under /usr/share/doc/mypackage/.\n";
        write_copyright(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert_eq!(
            fs::read_to_string(base.join("debian/copyright")).unwrap(),
            content
        );
    }
}
