use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction, OverrideLineSelector};
use crate::lintian_overrides::LintianOverrides;
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Return the package-relative paths of all lintian-overrides files in the
/// workspace, in a stable order: source overrides first, then per-binary
/// overrides sorted by filename.
fn find_override_files(ws: &dyn FixerWorkspace) -> Result<Vec<PathBuf>, FixerError> {
    let mut paths = Vec::new();
    let source_rel = PathBuf::from("debian/source/lintian-overrides");
    if ws.read_file(&source_rel)?.is_some() {
        paths.push(source_rel);
    }
    if let Some(mut entries) = ws.list_dir(Path::new("debian"))? {
        entries.sort();
        for name in entries {
            if name.ends_with(".lintian-overrides") {
                paths.push(PathBuf::from("debian").join(name));
            }
        }
    }
    Ok(paths)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rel in find_override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(&content);
        let Ok(overrides) = parsed.ok() else {
            continue;
        };

        // Group line numbers by (package, tag, info).
        let mut groups: HashMap<(Option<String>, String, String), Vec<usize>> = HashMap::new();
        for (idx, line) in overrides.lines().enumerate() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let package = line.package_spec().and_then(|s| s.package_name());
            let tag = line.tag().map(|t| t.text().to_string()).unwrap_or_default();
            let info = line
                .info()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            groups
                .entry((package, tag, info))
                .or_default()
                .push(idx + 1);
        }

        let mut duplicates: Vec<_> = groups.iter().filter(|(_, lines)| lines.len() > 1).collect();
        if duplicates.is_empty() {
            continue;
        }
        duplicates.sort_by_key(|(_, lines)| lines[0]);

        let override_file_path = rel
            .strip_prefix(Path::new("debian"))
            .unwrap_or(&rel)
            .to_string_lossy()
            .to_string();

        for ((package, tag, override_info), lines) in &duplicates {
            let lines_str = lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let info_parts = if override_info.is_empty() {
                vec![
                    tag.clone(),
                    format!("(lines {})", lines_str),
                    format!("[debian/{}]", override_file_path),
                ]
            } else {
                vec![
                    tag.clone(),
                    override_info.clone(),
                    format!("(lines {})", lines_str),
                    format!("[debian/{}]", override_file_path),
                ]
            };
            let info_for_selector = if override_info.is_empty() {
                None
            } else {
                Some(override_info.clone())
            };
            let selector = OverrideLineSelector {
                tag: tag.clone(),
                info: info_for_selector,
                package: package.clone(),
            };
            // Emit one diagnostic per duplicate group, but (n-1)
            // DropLine actions: each action consumes one matching line,
            // leaving the first occurrence intact.
            let drop_actions: Vec<Action> = (1..lines.len())
                .map(|_| {
                    Action::LintianOverrides(LintianOverridesAction::DropLine {
                        file: rel.clone(),
                        selector: selector.clone(),
                    })
                })
                .collect();
            let mut issue =
                LintianIssue::source_with_info("duplicate-override-context", info_parts);
            issue.package = package.clone();
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    "Remove duplicate lintian overrides.",
                    drop_actions,
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "duplicate-override-context",
    tags: ["duplicate-override-context"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_duplicate_in_source_overrides() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "# Comment\ntest-package source: some-tag info\ntest-package source: some-tag info\ntest-package source: other-tag\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "# Comment\ntest-package source: some-tag info\ntest-package source: other-tag\n",
        );
    }

    #[test]
    fn test_no_duplicates() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lintian-overrides"),
            "test-package source: tag1\ntest-package source: tag2\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_duplicates() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "pkg source: tag1\npkg source: tag1\npkg source: tag2 info\npkg source: tag2 info\npkg source: tag2 info\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "pkg source: tag1\npkg source: tag2 info\n",
        );
    }
}
