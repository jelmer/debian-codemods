use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

const TAG: &str = "composer-package-without-pkg-php-tools-builddep";
const REQUIRED_ANY: &[&str] = &["pkg-php-tools", "dh-sequence-phpcomposer"];
const ENTRY_TO_ADD: &str = "pkg-php-tools";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.read_file(Path::new("composer.json"))?.is_none() {
        return Ok(Vec::new());
    }

    // Lintian only emits this tag when there is no PEAR package.xml/package2.xml.
    if ws.read_file(Path::new("package.xml"))?.is_some()
        || ws.read_file(Path::new("package2.xml"))?.is_some()
    {
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

    let issue = LintianIssue::source_with_info(TAG, Visibility::Info, vec![]);
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Composer package without pkg-php-tools build-dependency.".to_string(),
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
    name: "composer-package-without-pkg-php-tools-builddep",
    tags: ["composer-package-without-pkg-php-tools-builddep"],
    triggers: [
        debian_workspace::Trigger::File("composer.json"),
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

    fn make_tree(composer: bool, pear_xml: Option<&str>, control: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), control).unwrap();
        if composer {
            fs::write(
                tmp.path().join("composer.json"),
                b"{\"name\":\"foo/bar\"}\n",
            )
            .unwrap();
        }
        if let Some(name) = pear_xml {
            fs::write(tmp.path().join(name), b"<package/>\n").unwrap();
        }
        tmp
    }

    #[test]
    fn test_adds_pkg_php_tools() {
        let tmp = make_tree(
            true,
            None,
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
            None,
            "Source: test\nBuild-Depends: debhelper (>= 13), pkg-php-tools\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_dh_sequence_phpcomposer_present() {
        let tmp = make_tree(
            true,
            None,
            "Source: test\nBuild-Depends: debhelper (>= 13), dh-sequence-phpcomposer\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_composer_json() {
        let tmp = make_tree(
            false,
            None,
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_pear_package_xml_present() {
        let tmp = make_tree(
            true,
            Some("package.xml"),
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_pear_package2_xml_present() {
        let tmp = make_tree(
            true,
            Some("package2.xml"),
            "Source: test\nBuild-Depends: debhelper (>= 13)\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("composer.json"), b"{}\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_only_binary_stanza() {
        let tmp = make_tree(
            true,
            None,
            "Package: test\nArchitecture: all\nDescription: foo\n foo.\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
