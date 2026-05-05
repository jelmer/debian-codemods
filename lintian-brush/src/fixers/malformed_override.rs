use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::lintian_overrides::{filter_overrides, LintianOverrides};
use crate::{FixerError, LintianIssue};
use std::path::{Path, PathBuf};

const REMOVED_TAGS: &[&str] = &[
    "hardening-no-stackprotector",
    "maintainer-not-full-name",
    "uploader-not-full-name",
    "uploader-address-missing",
    "no-upstream-changelog",
    "copyright-year-in-future",
    "script-calls-init-script-directly",
];

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

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut all_removed_tags: Vec<String> = Vec::new();

    for path in find_override_files(base_path) {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = LintianOverrides::parse(&content);
        let Ok(overrides) = parsed.ok() else {
            continue;
        };

        let mut removed_in_this_file: Vec<String> = Vec::new();
        let mut issues: Vec<LintianIssue> = Vec::new();
        for (lineno, line) in overrides.lines().enumerate() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let tag = tag_token.text();
            if !REMOVED_TAGS.contains(&tag) {
                continue;
            }
            let tag_string = tag.to_string();
            if !removed_in_this_file.contains(&tag_string) {
                removed_in_this_file.push(tag_string.clone());
            }
            if !all_removed_tags.contains(&tag_string) {
                all_removed_tags.push(tag_string);
            }
            let package_name = line.package_spec().and_then(|s| s.package_name());
            issues.push(if let Some(pkg) = package_name {
                LintianIssue::binary_with_info(
                    &pkg,
                    "malformed-override",
                    vec![format!("Unknown tag {} in line {}", tag, lineno + 1)],
                )
            } else {
                LintianIssue::source_with_info(
                    "malformed-override",
                    vec![format!("Unknown tag {} in line {}", tag, lineno + 1)],
                )
            });
        }
        if issues.is_empty() {
            continue;
        }

        let filtered = filter_overrides(&overrides, |line| {
            if line.is_comment() || line.is_empty() {
                return true;
            }
            line.tag()
                .map(|t| !REMOVED_TAGS.contains(&t.text()))
                .unwrap_or(true)
        });
        let new_content = filtered.text();
        let rel = path
            .strip_prefix(base_path)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let action = if new_content.trim().is_empty() {
            Action::Filesystem(FilesystemAction::Delete { file: rel })
        } else {
            Action::Filesystem(FilesystemAction::Write {
                file: rel,
                content: new_content.into_bytes(),
            })
        };
        for (i, issue) in issues.into_iter().enumerate() {
            let actions = if i == 0 {
                vec![action.clone()]
            } else {
                Vec::new()
            };
            diagnostics.push(Diagnostic::with_actions(issue, String::new(), actions));
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if all_removed_tags.is_empty() {
        "Remove overrides for lintian tags that are no longer supported".to_string()
    } else {
        format!(
            "Remove overrides for lintian tags that are no longer supported: {}",
            all_removed_tags.join(", ")
        )
    };
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

declare_fixer! {
    name: "malformed-override",
    tags: ["malformed-override"],
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
        let v: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_obsolete_tag() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "lintian-brush source: uploader-not-full-name\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove overrides for lintian tags that are no longer supported: uploader-not-full-name"
        );
        assert!(!overrides.exists());
    }

    #[test]
    fn test_keep_valid_tag() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "some-valid-tag\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&overrides).unwrap(), "some-valid-tag\n");
    }

    #[test]
    fn test_mixed_tags() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "valid-tag\nuploader-not-full-name\nanother-valid-tag\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "valid-tag\nanother-valid-tag\n",
        );
    }

    #[test]
    fn test_no_override_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
