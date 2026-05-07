use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::HashMap;
use std::path::PathBuf;

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
    let content = std::fs::read_to_string(LINTIAN_PYTHON_VERSIONS_PATH)?;
    let mut versions = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if let Ok(version) = parse_version(value.trim()) {
                versions.insert(key.trim().to_string(), version);
            }
        }
    }
    Ok(versions)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let python_versions = match load_python_versions() {
        Ok(v) => v,
        // Without lintian's data file we can't decide what's ancient.
        Err(_) => return Ok(Vec::new()),
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (field, threshold_key) in &[
        ("X-Python-Version", "old-python2"),
        ("X-Python3-Version", "old-python3"),
    ] {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let trimmed = value.trim();
        let Some(rest) = trimmed.strip_prefix(">=") else {
            continue;
        };
        let Ok(version) = parse_version(rest.trim()) else {
            continue;
        };
        let Some(&threshold) = python_versions.get(*threshold_key) else {
            continue;
        };
        if version > threshold {
            continue;
        }
        let issue = LintianIssue::source_with_info(
            "ancient-python-version-field",
            vec![format!("{}: {}", field, trimmed)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Ancient X-Python{,3}-Version field in debian/control.",
            "Remove unnecessary X-Python{,3}-Version field in debian/control.",
            vec![Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: (*field).to_string(),
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "ancient-python-version-field",
    tags: ["ancient-python-version-field", "old-python-version-field"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "X-Python-Version",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "X-Python3-Version",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "lintian-brush", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_ancient_python2_version() {
        if !std::path::Path::new(LINTIAN_PYTHON_VERSIONS_PATH).exists() {
            return; // Lintian data file not installed.
        }
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: lintian-brush\nX-Python-Version: >= 2.5\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_remove_ancient_python3_version() {
        if !std::path::Path::new(LINTIAN_PYTHON_VERSIONS_PATH).exists() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: lintian-brush\nX-Python3-Version: >= 3.2\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_no_change_when_no_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_recent_version() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: lintian-brush\nX-Python3-Version: >= 3.8\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(fs::read_to_string(&control)
            .unwrap()
            .contains("X-Python3-Version: >= 3.8"));
    }
}
