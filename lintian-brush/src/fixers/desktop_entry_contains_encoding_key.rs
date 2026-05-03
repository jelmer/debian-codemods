use crate::diagnostic::{Action, DesktopIniAction, Diagnostic};
use crate::{FixerError, LintianIssue};
use desktop_edit::Desktop;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debian_dir = base_path.join("debian");
    if !debian_dir.exists() {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();

    let entries = match fs::read_dir(&debian_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".desktop") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let desktop = Desktop::from_str(&content)
            .map_err(|e| FixerError::Other(format!("Failed to parse desktop file: {:?}", e)))?;
        let Some(group) = desktop.get_group("Desktop Entry") else {
            continue;
        };
        let Some(encoding) = group.get("Encoding") else {
            continue;
        };
        if encoding != "UTF-8" {
            continue;
        }

        let line_number = group
            .entries()
            .find(|e| e.key().as_deref() == Some("Encoding") && e.locale().is_none())
            .map(|e| e.line())
            .unwrap_or(0);

        let rel_path: PathBuf = path.strip_prefix(base_path).unwrap_or(&path).to_path_buf();
        let rel_str = rel_path.to_string_lossy().to_string();

        let issue = LintianIssue::source_with_info(
            "desktop-entry-contains-encoding-key",
            vec![
                "Encoding".to_string(),
                format!("[{}:{}]", rel_str, line_number),
            ],
        );

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!(
                "Remove deprecated Encoding key from desktop file {}.",
                rel_str
            ),
            vec![Action::DesktopIni(DesktopIniAction::RemoveField {
                file: rel_path,
                group: "Desktop Entry".into(),
                field: "Encoding".into(),
                locale: None,
            })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    if fixed.len() == 1 {
        return fixed[0].message.clone();
    }
    let paths: Vec<String> = fixed
        .iter()
        .filter_map(|d| {
            d.message
                .strip_prefix("Remove deprecated Encoding key from desktop file ")
                .and_then(|s| s.strip_suffix('.'))
                .map(|s| s.to_string())
        })
        .collect();
    format!(
        "Remove deprecated Encoding key from desktop files: {}.",
        paths.join(", ")
    )
}

declare_fixer! {
    name: "desktop-entry-contains-encoding-key",
    tags: ["desktop-entry-contains-encoding-key"],
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
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_utf8() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("foo.desktop");
        fs::write(
            &desktop_path,
            "[Desktop Entry]\nType=Application\nEncoding=UTF-8\nName=XScreensaver\nTryExec=xscreensaver\nExec=/usr/share/xscreensaver/xscreensaver-wrapper.sh -nosplash\nNoDisplay=true\nX-KDE-StartupNotify=false\nComment=The XScreensaver daemon\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove deprecated Encoding key from desktop file debian/foo.desktop."
        );

        assert_eq!(
            fs::read_to_string(&desktop_path).unwrap(),
            "[Desktop Entry]\nType=Application\nName=XScreensaver\nTryExec=xscreensaver\nExec=/usr/share/xscreensaver/xscreensaver-wrapper.sh -nosplash\nNoDisplay=true\nX-KDE-StartupNotify=false\nComment=The XScreensaver daemon\n"
        );
    }

    #[test]
    fn test_no_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_encoding_key() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("foo.desktop");
        fs::write(
            &desktop_path,
            "[Desktop Entry]\nType=Application\nName=Test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
