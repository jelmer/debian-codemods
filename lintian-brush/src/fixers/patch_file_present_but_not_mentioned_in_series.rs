use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use patchkit::quilt::{Series, SeriesEntry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SEP: char = '\t';

fn mentioned_in_comments(series: &Series) -> HashSet<String> {
    let mut mentioned = HashSet::new();
    for entry in series.iter() {
        if let SeriesEntry::Comment(comment) = entry {
            let comment = comment.trim_start_matches('#').trim();
            if let Some(word) = comment.split_whitespace().next() {
                mentioned.insert(word.to_string());
            }
        }
    }
    mentioned
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(series) = ws.parsed_patches_series()? else {
        return Ok(Vec::new());
    };

    let commented_out = mentioned_in_comments(&series);

    let mut entries = match ws.list_dir(Path::new("debian/patches"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();
    for name in entries {
        if name == "series" || name == "00list" {
            continue;
        }
        if name.starts_with("README") {
            continue;
        }
        // Skip directories (and any other non-readable entries) — only
        // proper patch files are candidates for removal.
        let rel = PathBuf::from("debian/patches").join(&name);
        if !matches!(ws.read_file(&rel), Ok(Some(_))) {
            continue;
        }
        if series.contains(&name) {
            continue;
        }
        if commented_out.contains(&name) {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "patch-file-present-but-not-mentioned-in-series",
            Visibility::Warning,
            vec![format!("[debian/patches/{}]", name)],
        );
        // Removing the file is destructive — in plenty of packages the
        // unreferenced patch is intentional (kept around for reference).
        // Only fire under --opinionated.
        diagnostics.push(Diagnostic::with_plans(
            issue,
            format!("file{}{}", SEP, name),
            vec![ActionPlan {
                label: format!("Remove unreferenced patch {}.", name),
                opinionated: true,
                certainty: None,
                actions: vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
            }],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut names: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| {
            d.message
                .split_once(SEP)
                .filter(|(tag, _)| *tag == "file")
                .map(|(_, name)| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    if names.len() == 1 {
        format!(
            "Remove patch {} that is missing from debian/patches/series.",
            names[0]
        )
    } else {
        format!(
            "Remove patches {} that are missing from debian/patches/series.",
            names.join(", ")
        )
    }
}

declare_detector! {
    name: "patch-file-present-but-not-mentioned-in-series",
    tags: ["patch-file-present-but-not-mentioned-in-series"],
    triggers: [
        debian_workspace::Trigger::File("debian/patches/series"),
        debian_workspace::Trigger::Glob("debian/patches/*"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let preferences = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &preferences)
        }
    }

    #[test]
    fn test_remove_unlisted_patch() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), "one\ntwo\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();
        fs::write(patches_dir.join("three"), "").unwrap();

        let result = run_apply(tmp.path(), true).unwrap();
        assert!(!patches_dir.join("three").exists());
        assert!(patches_dir.join("one").exists());
        assert!(patches_dir.join("two").exists());
        assert_eq!(
            result.description,
            "Remove patch three that is missing from debian/patches/series."
        );
    }

    #[test]
    fn test_no_changes_when_all_patches_listed() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), "one\ntwo\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();

        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_changes_when_not_opinionated() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();

        assert!(matches!(
            run_apply(tmp.path(), false),
            Err(FixerError::NoChanges)
        ));
        assert!(patches_dir.join("two").exists());
    }

    #[test]
    fn test_ignores_readme() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("README"), "").unwrap();
        fs::write(patches_dir.join("README.md"), "").unwrap();

        assert!(matches!(
            run_apply(tmp.path(), true),
            Err(FixerError::NoChanges)
        ));
        assert!(patches_dir.join("README").exists());
        assert!(patches_dir.join("README.md").exists());
    }

    #[test]
    fn test_multiple_patches_removed() {
        let tmp = TempDir::new().unwrap();
        let patches_dir = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();
        fs::write(patches_dir.join("series"), "one\n").unwrap();
        fs::write(patches_dir.join("one"), "").unwrap();
        fs::write(patches_dir.join("two"), "").unwrap();
        fs::write(patches_dir.join("three"), "").unwrap();

        let result = run_apply(tmp.path(), true).unwrap();
        assert_eq!(
            result.description,
            "Remove patches three, two that are missing from debian/patches/series."
        );
    }
}
