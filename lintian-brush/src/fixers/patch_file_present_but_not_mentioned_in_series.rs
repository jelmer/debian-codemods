use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{FixerError, FixerPreferences, LintianIssue};
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

pub fn detect(base_path: &Path, opinionated: bool) -> Result<Vec<Diagnostic>, FixerError> {
    if !opinionated {
        return Ok(Vec::new());
    }

    let series_abs = base_path.join("debian/patches/series");
    let patches_dir = base_path.join("debian/patches");

    let series = match std::fs::File::open(&series_abs) {
        Ok(file) => Series::read(file)
            .map_err(|e| FixerError::Other(format!("Failed to read series file: {}", e)))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let commented_out = mentioned_in_comments(&series);

    if !patches_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&patches_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut diagnostics = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name_os = entry.file_name();
        let Some(name) = file_name_os.to_str() else {
            continue;
        };
        if name == "series" || name == "00list" {
            continue;
        }
        if name.starts_with("README") {
            continue;
        }
        if series.contains(name) {
            continue;
        }
        if commented_out.contains(name) {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "patch-file-present-but-not-mentioned-in-series",
            vec![format!("[debian/patches/{}]", name)],
        );
        let rel = PathBuf::from("debian/patches").join(name);
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("file{}{}", SEP, name),
            vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
        ));
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut names: Vec<String> = fixed
        .iter()
        .filter_map(|d| {
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

declare_fixer! {
    name: "patch-file-present-but-not-mentioned-in-series",
    tags: ["patch-file-present-but-not-mentioned-in-series"],
    diagnose: |basedir, _package, _version, preferences: &FixerPreferences| {
        detect(basedir, preferences.opinionated.unwrap_or(false))
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, opinionated: bool) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let preferences = FixerPreferences {
            opinionated: Some(opinionated),
            ..Default::default()
        };
        FixerImpl.apply(base, "test", &version, &preferences)
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
