use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

/// The misspelt expansions lintian flags, paired with their corrected
/// form.
///
/// lintian's `debian/rules` check matches `\$[\(\{]DEB_BUILD_OPTS[\)\}]`,
/// i.e. only the `$(...)` and `${...}` expansions of the variable. A bare
/// `DEB_BUILD_OPTS` token (e.g. in an assignment) is a separate, un-flagged
/// thing, so the corrected delimiters are baked into each pattern rather
/// than substituting the bare name.
const SUBSTITUTIONS: [(&str, &str); 2] = [
    ("$(DEB_BUILD_OPTS)", "$(DEB_BUILD_OPTIONS)"),
    ("${DEB_BUILD_OPTS}", "${DEB_BUILD_OPTIONS}"),
];

const DESCRIPTION: &str =
    "debian/rules refers to $(DEB_BUILD_OPTS) instead of $(DEB_BUILD_OPTIONS).";
const LABEL: &str = "Use DEB_BUILD_OPTIONS rather than DEB_BUILD_OPTS in debian/rules.";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let bytes = match ws.read_file(Path::new("debian/rules"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    // Parse as a sanity check; if the file isn't a parseable makefile
    // there's nothing safe to substitute in.
    if ws.parsed_rules().is_err() {
        return Ok(Vec::new());
    }

    // lintian skips comment lines before matching, so a `$(DEB_BUILD_OPTS)`
    // that appears only inside a comment is not flagged. Keep only the
    // substitutions whose pattern occurs on at least one non-comment line.
    let actions: Vec<Action> = SUBSTITUTIONS
        .iter()
        .filter(|(from, _)| {
            content
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .any(|line| line.contains(from))
        })
        .map(|(from, to)| {
            Action::Filesystem(FilesystemAction::Substitute {
                file: PathBuf::from("debian/rules"),
                from: (*from).to_string(),
                to: (*to).to_string(),
            })
        })
        .collect();

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "debian-rules-uses-deb-build-opts",
        Visibility::Warning,
        vec!["[debian/rules]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        DESCRIPTION,
        LABEL,
        actions,
    )])
}

declare_detector! {
    name: "debian-rules-uses-deb-build-opts",
    tags: ["debian-rules-uses-deb-build-opts"],
    triggers: [debian_workspace::Trigger::File("debian/rules")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
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

    fn write_rules(base: &Path, contents: &str) -> PathBuf {
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("rules");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_replace_paren_form() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nifneq (,$(filter noopt,$(DEB_BUILD_OPTS)))\nCFLAGS += -O0\nendif\n\n%:\n\tdh $@\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, LABEL);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nifneq (,$(filter noopt,$(DEB_BUILD_OPTIONS)))\nCFLAGS += -O0\nendif\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_replace_brace_form() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nOPTS = ${DEB_BUILD_OPTS}\n\n%:\n\tdh $@\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nOPTS = ${DEB_BUILD_OPTIONS}\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_multiple_occurrences() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nA = $(DEB_BUILD_OPTS)\nB = $(DEB_BUILD_OPTS)\n\n%:\n\tdh $@\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nA = $(DEB_BUILD_OPTIONS)\nB = $(DEB_BUILD_OPTIONS)\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_both_forms() {
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nA = $(DEB_BUILD_OPTS)\nB = ${DEB_BUILD_OPTS}\n\n%:\n\tdh $@\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\nA = $(DEB_BUILD_OPTIONS)\nB = ${DEB_BUILD_OPTIONS}\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_comment_only_not_flagged() {
        // An occurrence inside a comment is what lintian skips; with no
        // real use of the variable there is nothing to fix.
        let tmp = TempDir::new().unwrap();
        let original =
            "#!/usr/bin/make -f\n\n# historically used $(DEB_BUILD_OPTS) here\n\n%:\n\tdh $@\n";
        let path = write_rules(tmp.path(), original);

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_comment_cleaned_up_alongside_real_use() {
        // Once a real line triggers the fix, the global substitution also
        // tidies up any stray occurrence in a comment.
        let tmp = TempDir::new().unwrap();
        let path = write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\n# see $(DEB_BUILD_OPTS) above\nA = $(DEB_BUILD_OPTS)\n\n%:\n\tdh $@\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#!/usr/bin/make -f\n\n# see $(DEB_BUILD_OPTIONS) above\nA = $(DEB_BUILD_OPTIONS)\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_already_correct() {
        let tmp = TempDir::new().unwrap();
        write_rules(
            tmp.path(),
            "#!/usr/bin/make -f\n\nA = $(DEB_BUILD_OPTIONS)\n\n%:\n\tdh $@\n",
        );

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_rules_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
