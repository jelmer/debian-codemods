use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const LINTIAN_PYTHON_VERSIONS_PATH: &str = "/usr/share/lintian/data/python/versions";

fn parse_version(version_str: &str) -> Result<(u8, u8), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = version_str.split('.').collect();
    if parts.len() != 2 {
        return Err("Invalid version format".into());
    }
    let major = parts[0].parse::<u8>()?;
    let minor = parts[1].parse::<u8>()?;
    Ok((major, minor))
}

fn load_python_versions() -> Result<HashMap<String, (u8, u8)>, FixerError> {
    let content = fs::read_to_string(LINTIAN_PYTHON_VERSIONS_PATH)?;
    let mut versions = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim();
            if let Ok(version) = parse_version(value) {
                versions.insert(key, version);
            }
        }
    }

    Ok(versions)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let python_versions = load_python_versions()?;

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Check X-Python-Version
        if let Some(x_python_version) = paragraph.get("X-Python-Version") {
            let x_python_version = x_python_version.trim();
            if x_python_version.starts_with(">=") {
                if let Some(vers_str) = x_python_version.strip_prefix(">=") {
                    let vers_str = vers_str.trim();
                    if let Ok(version) = parse_version(vers_str) {
                        // Check if it's old or ancient Python 2
                        if let Some(&old_python2) = python_versions.get("old-python2") {
                            if version <= old_python2 {
                                let issue = LintianIssue::source_with_info(
                                    "ancient-python-version-field",
                                    vec![format!("X-Python-Version: {}", x_python_version)],
                                );

                                if issue.should_fix(base_path) {
                                    paragraph.remove("X-Python-Version");
                                    fixed_issues.push(issue);
                                } else {
                                    overridden_issues.push(issue);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check X-Python3-Version
        if let Some(x_python3_version) = paragraph.get("X-Python3-Version") {
            let x_python3_version = x_python3_version.trim();
            if x_python3_version.starts_with(">=") {
                if let Some(vers_str) = x_python3_version.strip_prefix(">=") {
                    let vers_str = vers_str.trim();
                    if let Ok(version) = parse_version(vers_str) {
                        // Check if it's old or ancient Python 3
                        if let Some(&old_python3) = python_versions.get("old-python3") {
                            if version <= old_python3 {
                                let issue = LintianIssue::source_with_info(
                                    "ancient-python-version-field",
                                    vec![format!("X-Python3-Version: {}", x_python3_version)],
                                );

                                if issue.should_fix(base_path) {
                                    paragraph.remove("X-Python3-Version");
                                    fixed_issues.push(issue);
                                } else {
                                    overridden_issues.push(issue);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    Ok(
        FixerResult::builder("Remove unnecessary X-Python{,3}-Version field in debian/control.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "ancient-python-version-field",
    tags: ["ancient-python-version-field", "old-python-version-field"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_ancient_python2_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: lintian-brush\nX-Python-Version: >= 2.5\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("X-Python-Version"));
        assert!(updated_content.contains("Source: lintian-brush"));
    }

    #[test]
    fn test_remove_ancient_python3_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: lintian-brush\nX-Python3-Version: >= 3.2\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("X-Python3-Version"));
        assert!(updated_content.contains("Source: lintian-brush"));
    }

    #[test]
    fn test_no_change_when_no_field() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_recent_version() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: lintian-brush\nX-Python3-Version: >= 3.8\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // File should still contain the field
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("X-Python3-Version: >= 3.8"));
    }
}
