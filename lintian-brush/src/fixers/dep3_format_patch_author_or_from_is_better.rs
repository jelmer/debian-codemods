use crate::declare_detector;
use crate::diagnostic::{Action, Dep3Action, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use dep3::lossless::PatchHeader;
use patchkit::quilt::{Series, SeriesEntry};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let patches_rel = PathBuf::from("debian/patches");
    let series_bytes = match ws.read_file(&patches_rel.join("series"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let series = Series::read(&series_bytes[..])
        .map_err(|e| FixerError::Other(format!("Failed to read series file: {}", e)))?;

    let mut diagnostics = Vec::new();
    for entry in &series.entries {
        let SeriesEntry::Patch { name, .. } = entry else {
            continue;
        };
        let patch_rel_full = patches_rel.join(name);
        let Some(patch_bytes) = ws.read_file(&patch_rel_full)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&patch_bytes) else {
            continue;
        };

        let header_end = find_header_end(&content);
        let header_str = &content[..header_end];
        let Ok(header) = header_str.parse::<PatchHeader>() else {
            continue;
        };
        let Some((_category, origin)) = header.origin() else {
            continue;
        };
        let origin_str = origin.to_string();
        if !origin_str.contains('@') {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "dep3-format-patch-author-or-from-is-better",
            vec![format!("[debian/patches/{}]", name)],
        );
        let patch_rel = patches_rel.join(name);
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Patch header uses Origin where Author would be better.",
                "Use Author instead of Origin in patch headers.",
                vec![
                    Action::Dep3(Dep3Action::SetField {
                        file: patch_rel.clone(),
                        field: "Author".into(),
                        value: origin_str,
                    }),
                    Action::Dep3(Dep3Action::RemoveField {
                        file: patch_rel,
                        field: "Origin".into(),
                    }),
                ],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

fn find_header_end(content: &str) -> usize {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.starts_with("---")
            || trimmed.starts_with("diff ")
            || trimmed.starts_with("Index:")
        {
            return offset;
        }
        offset += line.len();
    }
    content.len()
}

declare_detector! {
    name: "dep3-format-patch-author-or-from-is-better",
    tags: ["dep3-format-patch-author-or-from-is-better"],
    triggers: [
        crate::workspace::Trigger::File("debian/patches/series"),
        crate::workspace::Trigger::Glob("debian/patches/*"),
    ],
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_replace_origin_with_author() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix-typo.patch\n").unwrap();
        let patch = patches.join("fix-typo.patch");
        fs::write(
            &patch,
            "Description: Fix a typo\nOrigin: john@example.com\nBug: https://example.com/bugs/123\n\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-teh\n+the\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&patch).unwrap();
        assert!(updated.contains("Author: john@example.com"));
        assert!(!updated.contains("Origin:"));
        assert!(updated.contains("--- a/file.txt"));
        assert!(updated.contains("+the"));
    }

    #[test]
    fn test_no_changes_when_origin_without_email() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix-typo.patch\n").unwrap();
        fs::write(
            patches.join("fix-typo.patch"),
            "Description: Fix a typo\nOrigin: upstream\n\n--- a/file.txt\n+++ b/file.txt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_origin() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix-typo.patch\n").unwrap();
        fs::write(
            patches.join("fix-typo.patch"),
            "Description: Fix a typo\nAuthor: jane@example.com\n\n--- a/file.txt\n+++ b/file.txt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_patches() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "patch1.patch\npatch2.patch\n").unwrap();
        fs::write(
            patches.join("patch1.patch"),
            "Description: Patch 1\nOrigin: user1@example.com\n\n--- a/file1.txt\n+++ b/file1.txt\n",
        )
        .unwrap();
        fs::write(
            patches.join("patch2.patch"),
            "Description: Patch 2\nOrigin: user2@example.com\n\n--- a/file2.txt\n+++ b/file2.txt\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        let p1 = fs::read_to_string(patches.join("patch1.patch")).unwrap();
        assert!(p1.contains("Author: user1@example.com") && !p1.contains("Origin:"));
        let p2 = fs::read_to_string(patches.join("patch2.patch")).unwrap();
        assert!(p2.contains("Author: user2@example.com") && !p2.contains("Origin:"));
    }
}
