use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::lintian_overrides::{filter_overrides, LintianOverrides};
use crate::{Certainty, FixerError, LintianIssue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn find_override_files(base_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        paths.push(source_overrides);
    }
    let debian_dir = base_path.join("debian");
    if let Ok(entries) = std::fs::read_dir(&debian_dir) {
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            let path = entry.path();
            if let Some(name) = path.file_name() {
                if name.to_string_lossy().ends_with(".lintian-overrides") {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn process_one_file(
    path: &Path,
    base_path: &Path,
) -> Result<Option<(Vec<LintianIssue>, String)>, FixerError> {
    let content = std::fs::read_to_string(path)?;
    let parsed = LintianOverrides::parse(&content);
    let overrides = parsed
        .ok()
        .map_err(|_| FixerError::Other("parse error".to_string()))?;

    let mut override_lines: HashMap<(Option<String>, String, String), Vec<usize>> = HashMap::new();
    let mut line_number = 0;
    for line in overrides.lines() {
        line_number += 1;
        if line.is_comment() || line.is_empty() {
            continue;
        }
        let package = line.package_spec().and_then(|s| s.package_name());
        let tag = line.tag().map(|t| t.text().to_string()).unwrap_or_default();
        let info = line.info().unwrap_or_default();
        override_lines
            .entry((package, tag, info))
            .or_default()
            .push(line_number);
    }

    let mut duplicates: Vec<_> = override_lines
        .iter()
        .filter(|(_, lines)| lines.len() > 1)
        .collect();
    if duplicates.is_empty() {
        return Ok(None);
    }
    duplicates.sort_by_key(|(_, lines)| lines[0]);

    let override_file_path = path
        .strip_prefix(base_path.join("debian"))
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let mut issues = Vec::new();
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
        let mut issue = LintianIssue::source_with_info("duplicate-override-context", info_parts);
        issue.package = package.clone();
        issues.push(issue);
    }

    // Filter to drop duplicates (keep first occurrence).
    let mut seen: HashMap<(Option<String>, String, String), bool> = HashMap::new();
    let filtered = filter_overrides(&overrides, |line| {
        if line.is_comment() || line.is_empty() {
            return true;
        }
        let package = line.package_spec().and_then(|s| s.package_name());
        let tag = line.tag().map(|t| t.text().to_string()).unwrap_or_default();
        let info = line.info().unwrap_or_default();
        let key = (package, tag, info);
        use std::collections::hash_map::Entry;
        match seen.entry(key) {
            Entry::Occupied(_) => false,
            Entry::Vacant(e) => {
                e.insert(true);
                true
            }
        }
    });

    Ok(Some((issues, filtered.to_string())))
}

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics = Vec::new();
    for path in find_override_files(base_path) {
        let Some((issues, new_content)) = process_one_file(&path, base_path)? else {
            continue;
        };
        let rel = path
            .strip_prefix(base_path)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let action = Action::Filesystem(FilesystemAction::Write {
            file: rel,
            content: new_content.into_bytes(),
        });
        for (idx, issue) in issues.into_iter().enumerate() {
            let actions = if idx == 0 {
                vec![action.clone()]
            } else {
                Vec::new()
            };
            diagnostics.push(
                Diagnostic::with_actions(issue, "Remove duplicate lintian overrides.", actions)
                    .with_certainty(Certainty::Certain),
            );
        }
    }
    Ok(diagnostics)
}

declare_fixer! {
    name: "duplicate-override-context",
    tags: ["duplicate-override-context"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
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
        FixerImpl.apply(base, "test-package", &version, &FixerPreferences::default())
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
