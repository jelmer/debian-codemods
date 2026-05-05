use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::relations::Relations;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const SEP: char = '\t';

fn has_misc_pre_depends(field_value: &str) -> bool {
    let (relations, _errors) = Relations::parse_relaxed(field_value, true);
    let found = relations.substvars().any(|s| s == "${misc:Pre-Depends}");
    found
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let compat_version = debian_analyzer::debhelper::get_debhelper_compat_level(base_path)?;
    if let Some(version) = compat_version {
        if version <= 11 {
            return Ok(Vec::new());
        }
    } else {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };

    let debian_dir = base_path.join("debian");
    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(package_name) = binary.as_deb822().get("Package") else {
            continue;
        };
        let init_path = debian_dir.join(format!("{}.init", package_name));
        let service_path = debian_dir.join(format!("{}.service", package_name));
        let upstart_path = debian_dir.join(format!("{}.upstart", package_name));
        if !init_path.exists() {
            continue;
        }
        if !service_path.exists() && !upstart_path.exists() {
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

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut packages: Vec<String> = fixed
        .iter()
        .filter_map(|d| {
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

declare_fixer! {
    name: "skip-systemd-native-flag-missing-pre-depends",
    tags: ["skip-systemd-native-flag-missing-pre-depends"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
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
