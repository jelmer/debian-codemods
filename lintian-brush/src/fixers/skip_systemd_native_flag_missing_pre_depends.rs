use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::{compat_level, FixerWorkspace};
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_control::lossless::relations::Relations;
use std::path::PathBuf;

const SEP: char = '\t';

fn has_misc_pre_depends(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);
    let found = relations.substvars().any(|s| s == "${misc:Pre-Depends}");
    found
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    match compat_level(ws)? {
        Some(version) if version > 11 => {}
        _ => return Ok(Vec::new()),
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(package_name) = binary.as_deb822().get("Package") else {
            continue;
        };
        let init_rel = PathBuf::from("debian").join(format!("{}.init", package_name));
        let service_rel = PathBuf::from("debian").join(format!("{}.service", package_name));
        let upstart_rel = PathBuf::from("debian").join(format!("{}.upstart", package_name));
        if ws.read_file(&init_rel)?.is_none() {
            continue;
        }
        if ws.read_file(&service_rel)?.is_none() && ws.read_file(&upstart_rel)?.is_none() {
            continue;
        }

        let pre_depends = binary
            .as_deb822()
            .get("Pre-Depends")
            .map(|s| s.to_string())
            .unwrap_or_default();
        if has_misc_pre_depends(&pre_depends) {
            continue;
        }

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "skip-systemd-native-flag-missing-pre-depends",
            vec![package_name.clone()],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("pkg{}{}", SEP, package_name),
            format!(
                "Add missing Pre-Depends: ${{misc:Pre-Depends}} in {}.",
                package_name
            ),
            vec![Action::Deb822(Deb822Action::EnsureSubstvar {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Pre-Depends".into(),
                substvar: "${misc:Pre-Depends}".into(),
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut packages: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| {
            d.message
                .split_once(SEP)
                .filter(|(tag, _)| *tag == "pkg")
                .map(|(_, pkg)| pkg.to_string())
        })
        .collect();
    packages.sort();
    packages.dedup();
    format!(
        "Add missing Pre-Depends: ${{misc:Pre-Depends}} in {}.",
        packages.join(", ")
    )
}

declare_detector! {
    name: "skip-systemd-native-flag-missing-pre-depends",
    tags: ["skip-systemd-native-flag-missing-pre-depends"],
    triggers: [
        crate::workspace::Trigger::File("debian/compat"),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Pre-Depends",
        },
        crate::workspace::Trigger::Glob("debian/*.init"),
        crate::workspace::Trigger::Glob("debian/*.service"),
        crate::workspace::Trigger::Glob("debian/*.upstart"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
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
    fn test_add_misc_pre_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 12)\n\nPackage: blah\nDescription: description\n longer description\n",
        )
        .unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        fs::write(debian.join("blah.init"), "").unwrap();
        fs::write(debian.join("blah.service"), "").unwrap();

        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(debian.join("control")).unwrap();
        assert!(updated.contains("Pre-Depends: ${misc:Pre-Depends}"));
    }

    #[test]
    fn test_already_has_misc_pre_depends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 12)\n\nPackage: blah\nPre-Depends: ${misc:Pre-Depends}\nDescription: description\n longer description\n",
        )
        .unwrap();
        fs::write(debian.join("compat"), "12\n").unwrap();
        fs::write(debian.join("blah.init"), "").unwrap();
        fs::write(debian.join("blah.service"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_compat_level_too_old() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 11)\n\nPackage: blah\nDescription: description\n longer description\n",
        )
        .unwrap();
        fs::write(debian.join("compat"), "11\n").unwrap();
        fs::write(debian.join("blah.init"), "").unwrap();
        fs::write(debian.join("blah.service"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
