use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::collections::BTreeSet;
use std::path::PathBuf;

const SEP: char = '\t';

/// Which VCS field types can carry `-b <branch>` and `[subpath]` annotations.
///
/// Only these can suffer from `--branch` typo. Other Vcs-* platforms use
/// bare URIs and don't take branch suffixes.
const VCS_BRANCH_TYPES: &[&str] = &["Git", "Bzr"];

/// Attempt to normalise a Vcs field value that has unexpected internal
/// whitespace. Returns `Some(new_value)` if a high-confidence rewrite is
/// possible, `None` otherwise.
///
/// The common case is `--branch <foo>` where the maintainer used the long
/// git-clone option instead of `-b <foo>` that lintian recognises. Anything
/// else with stray whitespace is ambiguous and left for a human.
pub fn fix_vcs_field(value: &str) -> Option<String> {
    if !has_unexpected_spaces(value) {
        return None;
    }
    let fixed = rewrite_long_branch(value)?;
    if has_unexpected_spaces(&fixed) {
        return None;
    }
    Some(fixed)
}

/// Rewrite ` --branch <name>` to ` -b <name>` in a Vcs field value.
///
/// Only rewrites when there is exactly one such occurrence and it sits at
/// the end of the value (possibly followed by a `[subpath]`). Returns
/// `None` if the pattern does not match cleanly.
fn rewrite_long_branch(value: &str) -> Option<String> {
    let idx = value.find(" --branch ")?;
    if value[idx + 1..].find(" --branch ").is_some() {
        return None;
    }
    let before = &value[..idx];
    let after = &value[idx + " --branch ".len()..];
    let (branch, tail) = match after.find(' ') {
        Some(sp) => (&after[..sp], &after[sp..]),
        None => (after, ""),
    };
    if branch.is_empty() {
        return None;
    }
    Some(format!("{} -b {}{}", before, branch, tail))
}

/// Reproduce lintian's `vcs-field-has-unexpected-spaces` detection: parse
/// the value with the same regexes lintian uses and report `true` when
/// any resulting part still contains whitespace.
///
/// This intentionally mirrors `Lintian::Check::Fields::Vcs::VCS_EXTRACT`
/// (`lib/Lintian/Check/Fields/Vcs.pm`).
fn has_unexpected_spaces(value: &str) -> bool {
    let re_subpath = regex::Regex::new(r" \[([^] ]+)\]").unwrap();
    let stripped = re_subpath.replace(value, "").to_string();
    let (url, branch) = match stripped.find(" -b ") {
        Some(idx) => (&stripped[..idx], Some(&stripped[idx + 4..])),
        None => (stripped.as_str(), None),
    };
    if url.contains(char::is_whitespace) {
        return true;
    }
    if let Some(b) = branch {
        if b.contains(char::is_whitespace) {
            return true;
        }
    }
    false
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
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let para = source.as_deb822();

    let mut diagnostics = Vec::new();
    for vcs_type in VCS_BRANCH_TYPES {
        let field = format!("Vcs-{}", vcs_type);
        let Some(value) = para.get(&field) else {
            continue;
        };
        if !has_unexpected_spaces(&value) {
            continue;
        }
        let Some(new_value) = fix_vcs_field(&value) else {
            continue;
        };
        let issue = LintianIssue::source_with_info(
            "vcs-field-has-unexpected-spaces",
            Visibility::Warning,
            vec![vcs_type.to_string(), value.clone()],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("field{}{}", SEP, field),
            format!("Remove unexpected spaces from {}.", field),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: field.clone(),
                value: new_value,
            })],
        ));
    }
    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut fields: BTreeSet<String> = BTreeSet::new();
    for (d, _) in fixed {
        let parts: Vec<&str> = d.message.split(SEP).collect();
        if parts.len() == 2 && parts[0] == "field" {
            fields.insert(parts[1].to_string());
        }
    }
    if fields.len() == 1 {
        format!(
            "Remove unexpected spaces from {}.",
            fields.iter().next().unwrap()
        )
    } else {
        format!(
            "Remove unexpected spaces from Vcs control headers: {}.",
            fields.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

declare_detector! {
    name: "vcs-field-has-unexpected-spaces",
    tags: ["vcs-field-has-unexpected-spaces"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test-package".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn detects_double_dash_branch() {
        assert!(has_unexpected_spaces(
            "https://example.com/foo.git --branch main"
        ));
    }

    #[test]
    fn accepts_clean_value() {
        assert!(!has_unexpected_spaces("https://example.com/foo.git"));
        assert!(!has_unexpected_spaces(
            "https://example.com/foo.git -b main"
        ));
        assert!(!has_unexpected_spaces(
            "https://example.com/foo.git -b main [sub]"
        ));
        assert!(!has_unexpected_spaces(
            "https://example.com/foo.git [sub] -b main"
        ));
    }

    #[test]
    fn detects_bare_space_in_url() {
        assert!(has_unexpected_spaces("https://example.com/foo bar"));
    }

    #[test]
    fn detects_space_in_branch_name() {
        assert!(has_unexpected_spaces(
            "https://example.com/foo.git -b main branch"
        ));
    }

    #[test]
    fn rewrites_long_branch() {
        assert_eq!(
            fix_vcs_field("https://example.com/foo.git --branch main"),
            Some("https://example.com/foo.git -b main".to_string())
        );
    }

    #[test]
    fn rewrites_long_branch_with_subpath() {
        assert_eq!(
            fix_vcs_field("https://example.com/foo.git --branch main [sub]"),
            Some("https://example.com/foo.git -b main [sub]".to_string())
        );
    }

    #[test]
    fn does_not_rewrite_clean_value() {
        assert_eq!(fix_vcs_field("https://example.com/foo.git"), None);
    }

    #[test]
    fn backs_off_on_ambiguous_bare_space() {
        assert_eq!(fix_vcs_field("https://example.com/foo bar/baz.git"), None);
    }

    #[test]
    fn backs_off_on_bare_branch_word() {
        assert_eq!(
            fix_vcs_field("https://example.com/foo.git wrong-branch"),
            None
        );
    }

    #[test]
    fn fixes_control_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nVcs-Git: https://example.com/foo.git --branch main\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Remove unexpected spaces from Vcs-Git.");
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nVcs-Git: https://example.com/foo.git -b main\n\nPackage: test-package\nDescription: Test\n Test test\n",
        );
    }

    #[test]
    fn no_changes_when_clean() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: https://example.com/foo.git -b main\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn no_changes_when_no_fix_available() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: https://example.com/foo.git wrong-branch\n\nPackage: test-package\nDescription: Test\n Test test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
