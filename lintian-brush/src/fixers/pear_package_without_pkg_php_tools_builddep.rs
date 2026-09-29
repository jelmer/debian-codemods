use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const TAG: &str = "pear-package-without-pkg-php-tools-builddep";
const REQUIRED_ANY: &[&str] = &["pkg-php-tools", "dh-sequence-phppear"];
const ENTRY_TO_ADD: &str = "pkg-php-tools";

fn has_php_file(ws: &dyn Workspace) -> Result<bool, debian_workspace::Error> {
    let Some(entries) = ws.walk_dir(Path::new("."))? else {
        return Ok(false);
    };
    for rel in entries {
        let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".php") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let has_pkg_xml = ws.read_file(Path::new("package.xml"))?.is_some()
        || ws.read_file(Path::new("package2.xml"))?.is_some();
    if !has_pkg_xml {
        return Ok(Vec::new());
    }

    if !has_php_file(ws)? {
        return Ok(Vec::new());
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    if let Some(build_depends) = source.build_depends() {
        for required in REQUIRED_ANY {
            if build_depends.get_relation(required).is_ok() {
                return Ok(Vec::new());
            }
        }
    }

    let issue = LintianIssue::source_with_info(TAG, Visibility::Warning, vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "PEAR package without pkg-php-tools build-dependency.".to_string(),
        format!("Add {} to Build-Depends.", ENTRY_TO_ADD),
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: ENTRY_TO_ADD.into(),
        })],
    )])
}

declare_detector! {
    name: "pear-package-without-pkg-php-tools-builddep",
    tags: ["pear-package-without-pkg-php-tools-builddep"],
    triggers: [
        debian_workspace::Trigger::File("package.xml"),
        debian_workspace::Trigger::File("package2.xml"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
    ],
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

    fn make_tree(pkg_xml: bool, php_file: bool, control: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), control).unwrap();
        if pkg_xml {
            fs::write(tmp.path().join("package.xml"), b"<package/>\n").unwrap();
        }
        if php_file {
            fs::write(tmp.path().join("index.php"), b"<?php echo 1; ?>\n").unwrap();
        }
        tmp
    }

    #[test]
    fn test_adds_pkg_php_tools() {
        let tmp = make_tree(
            true,
            true,
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Add pkg-php-tools to Build-Depends.");
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\nBuild-Depends: debhelper (>= 13), pkg-php-tools\n\nPackage: test\nArchitecture: all\n",
        );
    }

    #[test]
    fn test_no_change_when_pkg_php_tools_already_present() {
        let tmp = make_tree(
            true,
            true,
            "Source: test\nBuild-Depends: debhelper (>= 13), pkg-php-tools\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_dh_sequence_phppear_present() {
        let tmp = make_tree(
            true,
            true,
            "Source: test\nBuild-Depends: debhelper (>= 13), dh-sequence-phppear\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_package_xml() {
        let tmp = make_tree(
            false,
            true,
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_php_file() {
        let tmp = make_tree(
            true,
            false,
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_package2_xml_works() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        )
        .unwrap();
        fs::write(tmp.path().join("package2.xml"), b"<package/>\n").unwrap();
        fs::write(tmp.path().join("main.php"), b"<?php ?>\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(fs::read_to_string(tmp.path().join("debian/control"))
            .unwrap()
            .contains("pkg-php-tools"));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.xml"), b"<package/>\n").unwrap();
        fs::write(tmp.path().join("a.php"), b"<?php ?>\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
