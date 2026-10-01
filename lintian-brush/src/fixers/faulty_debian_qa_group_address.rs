use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_changelog::parseaddr;
use debian_workspace::Workspace;
use std::path::PathBuf;

const OLD_QA_ADDRESS: &str = "debian-qa@lists.debian.org";
const QA_ADDRESS: &str = "packages@qa.debian.org";
const QA_PHRASE: &str = "Debian QA Group";

/// Return the replacement address if `value` should be fixed, otherwise `None`.
///
/// Mirrors the lintian check: trigger when either the address is the old
/// `debian-qa@lists.debian.org` list, or when the phrase mentions "debian qa"
/// but the address is not the canonical `packages@qa.debian.org`.
fn needs_fix(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (phrase_opt, address) = parseaddr(trimmed);
    if address.is_empty() {
        return None;
    }
    // Lintian's regex is /\bdebian\s+qa\b/i: "debian" and "qa" as adjacent,
    // whitespace-separated, case-insensitive word tokens.
    let phrase_matches_qa = phrase_opt
        .map(|p| {
            let lower = p.to_ascii_lowercase();
            let words: Vec<&str> = lower.split_whitespace().collect();
            words
                .windows(2)
                .any(|pair| pair[0] == "debian" && pair[1] == "qa")
        })
        .unwrap_or(false);

    if address == OLD_QA_ADDRESS {
        return Some(format!("{} <{}>", QA_PHRASE, QA_ADDRESS));
    }
    if phrase_matches_qa && address != QA_ADDRESS {
        return Some(format!("{} <{}>", QA_PHRASE, QA_ADDRESS));
    }
    None
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
    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };
    let Some(new_value) = needs_fix(&maintainer) else {
        return Ok(Vec::new());
    };

    let (_, old_address) = parseaddr(maintainer.trim());

    let issue = LintianIssue::source_with_info(
        "faulty-debian-qa-group-address",
        Visibility::Error,
        vec![format!("Maintainer {} -> {}", old_address, QA_ADDRESS)],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Maintainer uses old Debian QA group address.",
        "Fix Debian QA group address.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: new_value,
        })],
    )])
}

declare_detector! {
    name: "faulty-debian-qa-group-address",
    tags: ["faulty-debian-qa-group-address"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base_path: &Path, maintainer: &str) {
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("control"),
            format!(
                "Source: test\nMaintainer: {}\n\nPackage: test\nDescription: Test\n Test package\n",
                maintainer
            ),
        )
        .unwrap();
    }

    #[test]
    fn needs_fix_old_qa_list_address() {
        assert_eq!(
            needs_fix("Debian QA Group <debian-qa@lists.debian.org>"),
            Some("Debian QA Group <packages@qa.debian.org>".into()),
        );
    }

    #[test]
    fn needs_fix_qa_phrase_wrong_address() {
        assert_eq!(
            needs_fix("Debian QA Team <team@example.com>"),
            Some("Debian QA Group <packages@qa.debian.org>".into()),
        );
    }

    #[test]
    fn needs_fix_canonical_address_is_fine() {
        assert_eq!(needs_fix("Debian QA Group <packages@qa.debian.org>"), None,);
    }

    #[test]
    fn needs_fix_unrelated_maintainer_is_fine() {
        assert_eq!(needs_fix("John Doe <john@example.com>"), None);
    }

    #[test]
    fn needs_fix_phrase_mentions_debian_but_not_qa() {
        assert_eq!(needs_fix("Debian Perl Group <perl@example.com>"), None);
    }

    #[test]
    fn test_fix_old_list_address() {
        let temp_dir = TempDir::new().unwrap();
        write_control(
            temp_dir.path(),
            "Some Maintainer <debian-qa@lists.debian.org>",
        );

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.description, "Fix Debian QA group address.");
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("debian/control")).unwrap(),
            "Source: test\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_fix_qa_phrase_wrong_address() {
        let temp_dir = TempDir::new().unwrap();
        write_control(temp_dir.path(), "Debian QA Team <foo@example.com>");

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.description, "Fix Debian QA group address.");
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("debian/control")).unwrap(),
            "Source: test\nMaintainer: Debian QA Group <packages@qa.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_no_change_canonical_address() {
        let temp_dir = TempDir::new().unwrap();
        write_control(temp_dir.path(), "Debian QA Group <packages@qa.debian.org>");

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert!(detect_in(temp_dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_regular_maintainer() {
        let temp_dir = TempDir::new().unwrap();
        write_control(temp_dir.path(), "Jane Doe <jane@example.com>");

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_change_other_debian_group() {
        let temp_dir = TempDir::new().unwrap();
        write_control(temp_dir.path(), "Debian Perl Group <perl@example.com>");

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert!(detect_in(temp_dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_no_maintainer_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test\n\nPackage: test\nDescription: Test\n Test\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_diagnostic_carries_action() {
        let temp_dir = TempDir::new().unwrap();
        write_control(
            temp_dir.path(),
            "Some Maintainer <debian-qa@lists.debian.org>",
        );

        let diagnostics = detect_in(temp_dir.path()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        assert_eq!(diag.message, "Maintainer uses old Debian QA group address.");
        let info = diag.issue.as_ref().and_then(|i| i.info.as_deref());
        assert_eq!(
            info,
            Some("Maintainer debian-qa@lists.debian.org -> packages@qa.debian.org"),
        );
        assert_eq!(diag.plans.len(), 1);
        assert_eq!(diag.plans[0].actions.len(), 1);
        assert_eq!(
            diag.plans[0].actions[0],
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Maintainer".into(),
                value: format!("{} <{}>", QA_PHRASE, QA_ADDRESS),
            }),
        );
    }

    #[test]
    fn test_overridden_diagnostic() {
        let temp_dir = TempDir::new().unwrap();
        write_control(
            temp_dir.path(),
            "Some Maintainer <debian-qa@lists.debian.org>",
        );
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(debian_dir.join("source")).unwrap();
        fs::write(
            debian_dir.join("source/lintian-overrides"),
            "faulty-debian-qa-group-address\n",
        )
        .unwrap();

        match run_apply(temp_dir.path()).unwrap_err() {
            FixerError::NoChangesAfterOverrides(issues) => {
                assert_eq!(issues.len(), 1);
                assert_eq!(
                    issues[0].tag.as_deref(),
                    Some("faulty-debian-qa-group-address"),
                );
            }
            other => panic!("expected NoChangesAfterOverrides, got {:?}", other),
        }
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("debian/control")).unwrap(),
            "Source: test\nMaintainer: Some Maintainer <debian-qa@lists.debian.org>\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }
}
