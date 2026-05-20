use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.read_file(Path::new("debian/debcargo.toml"))?.is_some() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let current = source.as_deb822().get("Rules-Requires-Root");

    let compat_release = preferences.compat_release.as_deref().unwrap_or("sid");
    let Some(oldest_dpkg) = debian_analyzer::release_info::dpkg_versions
        .get(compat_release)
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let dpkg_1_22_13 = debversion::Version::from_str("1.22.13").unwrap();

    if current.is_none() {
        if oldest_dpkg < dpkg_1_22_13 {
            // TODO: heuristics for setting "yes" when debian/rules chowns
            // files; for now, always default to "no".
            let issue = LintianIssue::source_with_info(
                "silent-on-rules-requiring-root",
                Visibility::Warning,
                vec!["[debian/control]".to_string()],
            );
            return Ok(vec![Diagnostic::with_actions(
                issue,
                "Rules-Requires-Root field is missing.",
                "Set Rules-Requires-Root: no.",
                vec![Action::Deb822(Deb822Action::SetField {
                    file: control_rel,
                    paragraph: ParagraphSelector::Source,
                    field: "Rules-Requires-Root".into(),
                    value: "no".into(),
                })],
            )
            .with_certainty(Certainty::Possible)]);
        }
    } else if current.as_deref() == Some("no") && oldest_dpkg >= dpkg_1_22_13 {
        let issue = LintianIssue::source_with_info(
            "redundant-rules-requires-root-no-field",
            Visibility::Pedantic,
            vec!["[debian/control]".to_string()],
        );
        return Ok(vec![Diagnostic::with_actions(
            issue,
            "Rules-Requires-Root: no is redundant on modern dpkg.",
            "Removed Rules-Requires-Root",
            vec![Action::Deb822(Deb822Action::RemoveField {
                file: control_rel,
                paragraph: ParagraphSelector::Source,
                field: "Rules-Requires-Root".into(),
            })],
        )
        .with_certainty(Certainty::Possible)]);
    }

    Ok(Vec::new())
}

declare_detector! {
    name: "rules-requires-root-missing",
    tags: [
        "silent-on-rules-requiring-root",
        "redundant-rules-requires-root-no-field",
    ],
    triggers: [
        debian_workspace::Trigger::File("debian/debcargo.toml"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Rules-Requires-Root",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, preferences)
        }
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        let prefs = FixerPreferences::default();
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_debcargo_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("debcargo.toml"), "").unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\n\nPackage: test-package\n",
        )
        .unwrap();

        let prefs = FixerPreferences::default();
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_add_rules_requires_root_old_dpkg() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            compat_release: Some("bullseye".to_string()),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &prefs).unwrap();
        assert_eq!(result.description, "Set Rules-Requires-Root: no.");
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nMaintainer: Test <test@example.com>\nRules-Requires-Root: no\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        );
    }

    #[test]
    fn test_no_change_new_dpkg() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            compat_release: Some("trixie".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_remove_rules_requires_root_new_dpkg() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: test-package\nRules-Requires-Root: no\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            compat_release: Some("trixie".to_string()),
            ..Default::default()
        };
        let result = run_apply(tmp.path(), &prefs).unwrap();
        assert_eq!(result.description, "Removed Rules-Requires-Root");
        assert_eq!(
            result.fixed_lintian_tags(),
            vec!["redundant-rules-requires-root-no-field"],
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        );
    }

    #[test]
    fn test_no_remove_rules_requires_root_yes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nRules-Requires-Root: yes\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: all\nDescription: Test package\n test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            compat_release: Some("trixie".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }
}
