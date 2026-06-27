use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, DesktopIniAction, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use desktop_edit::Desktop;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Keys deprecated by the FreeDesktop Desktop Entry specification.
///
/// Mirrors lintian's `data/menu-format/deprecated-desktop-keys`. `Encoding`
/// is deliberately excluded; lintian reports it under the separate
/// `desktop-entry-contains-encoding-key` tag, handled by its own fixer.
const DEPRECATED_KEYS: &[&str] = &[
    "BinaryPattern",
    "Extensions",
    "FilePattern",
    "MapNotify",
    "MiniIcon",
    "Protocols",
    "SortOrder",
    "SwallowExec",
    "SwallowTitle",
    "TerminalOptions",
];

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();

    for name in entries {
        if !name.ends_with(".desktop") {
            continue;
        }
        let rel_path = PathBuf::from("debian").join(&name);
        let Some(bytes) = ws.read_file(&rel_path)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let desktop = Desktop::from_str(content)
            .map_err(|e| FixerError::Other(format!("Failed to parse desktop file: {:?}", e)))?;

        // lintian only inspects the first group, and only if it is the
        // [Desktop Entry] (or [KDE Desktop Entry]) header.
        let Some(group) = desktop.groups().next() else {
            continue;
        };
        let group_name = match group.name() {
            Some(n) if n == "Desktop Entry" || n == "KDE Desktop Entry" => n,
            _ => continue,
        };

        let rel_str = rel_path.to_string_lossy().to_string();

        for entry in group.entries() {
            let Some(key) = entry.key() else {
                continue;
            };
            if !DEPRECATED_KEYS.contains(&key.as_str()) {
                continue;
            }

            // lintian reports the key as written, including any locale suffix.
            let locale = entry.locale();
            let tag_info = match &locale {
                Some(loc) => format!("{}[{}]", key, loc),
                None => key.clone(),
            };

            let issue = LintianIssue::source_with_info(
                "desktop-entry-contains-deprecated-key",
                Visibility::Warning,
                vec![tag_info.clone(), format!("[{}:{}]", rel_str, entry.line())],
            );

            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "Desktop file {} contains deprecated key {}.",
                    rel_str, tag_info
                ),
                format!(
                    "Remove deprecated key {} from desktop file {}.",
                    tag_info, rel_str
                ),
                vec![Action::DesktopIni(DesktopIniAction::RemoveField {
                    file: rel_path.clone(),
                    group: group_name.clone(),
                    field: key.clone(),
                    locale,
                })],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    if fixed.len() == 1 {
        return fixed[0]
            .0
            .plans
            .first()
            .map(|p| p.label.clone())
            .unwrap_or_default();
    }
    format!("Remove {} deprecated keys from desktop files.", fixed.len())
}

declare_detector! {
    name: "desktop-entry-contains-deprecated-key",
    tags: ["desktop-entry-contains-deprecated-key"],
    triggers: [debian_workspace::Trigger::Glob("debian/*.desktop")],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_desktop(base: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let debian_dir = base.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        let path = debian_dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_removes_deprecated_key() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nName=Foo\nSwallowExec=bar\nExec=/usr/bin/foo\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Remove deprecated key SwallowExec from desktop file debian/foo.desktop."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Desktop Entry]\nType=Application\nName=Foo\nExec=/usr/bin/foo\n"
        );
    }

    #[test]
    fn test_info_field() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nMiniIcon=foo\n",
        );

        let result = run_apply(base).unwrap();
        let issue = &result.fixed_lintian_issues[0];
        assert_eq!(
            issue.tag.as_deref(),
            Some("desktop-entry-contains-deprecated-key")
        );
        assert_eq!(
            issue.info.as_deref(),
            Some("MiniIcon [debian/foo.desktop:2]")
        );
    }

    #[test]
    fn test_locale_variant() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nSwallowTitle[de]=Hallo\nName=Foo\n",
        );

        let result = run_apply(base).unwrap();
        let issue = &result.fixed_lintian_issues[0];
        assert_eq!(
            issue.info.as_deref(),
            Some("SwallowTitle[de] [debian/foo.desktop:2]")
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Desktop Entry]\nType=Application\nName=Foo\n"
        );
    }

    #[test]
    fn test_multiple_keys() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nSortOrder=a\nName=Foo\nProtocols=http\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Desktop Entry]\nType=Application\nName=Foo\n"
        );
    }

    #[test]
    fn test_kde_desktop_entry() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let path = write_desktop(
            base,
            "foo.desktop",
            "[KDE Desktop Entry]\nType=Application\nMapNotify=true\nName=Foo\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[KDE Desktop Entry]\nType=Application\nName=Foo\n"
        );
    }

    #[test]
    fn test_encoding_not_handled() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nEncoding=UTF-8\nName=Foo\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_deprecated_key_in_other_group_ignored() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nName=Foo\n\n[Some Other Group]\nSwallowExec=bar\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_deprecated_key() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_desktop(
            base,
            "foo.desktop",
            "[Desktop Entry]\nType=Application\nName=Foo\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir(temp_dir.path().join("debian")).unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
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
