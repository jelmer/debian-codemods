use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.list_dir(Path::new("debian"))?.is_none() {
        return Ok(Vec::new());
    }

    // Source path → relative-path string used in the lintian issue info
    // and the human-readable message.
    let candidates: &[(PathBuf, &str)] = &[
        (PathBuf::from("debian/upstream"), "debian/upstream"),
        (
            PathBuf::from("debian/upstream-metadata"),
            "debian/upstream-metadata",
        ),
        (
            PathBuf::from("debian/upstream-metadata.yaml"),
            "debian/upstream-metadata.yaml",
        ),
    ];
    let target_rel = PathBuf::from("debian/upstream/metadata");

    let mut diagnostics = Vec::new();
    for (rel, label) in candidates {
        let content = match ws.read_file(rel) {
            // File: candidate.
            Ok(Some(c)) => c,
            // Missing or directory or other error → skip.
            _ => continue,
        };

        let issue = LintianIssue::source_with_info(
            "debian-upstream-obsolete-path",
            Visibility::Error,
            vec![format!("[{}]", label)],
        );

        // Special case: the legacy `debian/upstream` (a single file)
        // can't be `rename`d to `debian/upstream/metadata`, since the
        // destination's parent directory is the source itself. Read the
        // content, delete the file, then write to the new path.
        let actions = if rel == &PathBuf::from("debian/upstream") {
            vec![
                Action::Filesystem(FilesystemAction::Delete { file: rel.clone() }),
                Action::Filesystem(FilesystemAction::Write {
                    file: target_rel.clone(),
                    content: content.into_owned(),
                }),
            ]
        } else {
            vec![Action::Filesystem(FilesystemAction::Rename {
                file: rel.clone(),
                to: target_rel.clone(),
            })]
        };

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!("Upstream metadata is at obsolete path {}.", label),
                "Move upstream metadata to debian/upstream/metadata.",
                actions,
            )
            .with_certainty(Certainty::Certain),
        );

        // Only handle one file per run — the lintian-overrides semantics
        // work best with one diagnostic per fix.
        break;
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-upstream-obsolete-path",
    tags: ["debian-upstream-obsolete-path"],
    triggers: [
        debian_workspace::Trigger::File("debian/upstream"),
        debian_workspace::Trigger::File("debian/upstream-metadata"),
        debian_workspace::Trigger::File("debian/upstream-metadata.yaml"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_move_upstream_file_to_metadata() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();

        let upstream_file = debian.join("upstream");
        fs::write(
            &upstream_file,
            "Name: test\nRepository: git://example.com/test\n",
        )
        .unwrap();

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Move upstream metadata to debian/upstream/metadata."
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));

        // debian/upstream is now a directory (created by the Write action's
        // create_dir_all of the parent), and debian/upstream/metadata
        // contains the original content.
        assert!(upstream_file.is_dir());
        assert_eq!(
            fs::read_to_string(debian.join("upstream/metadata")).unwrap(),
            "Name: test\nRepository: git://example.com/test\n",
        );
    }

    #[test]
    fn test_move_upstream_metadata_file() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();

        let upstream_metadata = debian.join("upstream-metadata");
        fs::write(&upstream_metadata, "Name: test2\n").unwrap();

        let result = run_apply(base).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert!(!upstream_metadata.exists());
        assert_eq!(
            fs::read_to_string(debian.join("upstream/metadata")).unwrap(),
            "Name: test2\n",
        );
    }

    #[test]
    fn test_move_upstream_metadata_yaml_file() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();

        let upstream_metadata_yaml = debian.join("upstream-metadata.yaml");
        fs::write(&upstream_metadata_yaml, "Name: test3\n").unwrap();

        let result = run_apply(base).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));

        assert!(!upstream_metadata_yaml.exists());
        assert_eq!(
            fs::read_to_string(debian.join("upstream/metadata")).unwrap(),
            "Name: test3\n",
        );
    }

    #[test]
    fn test_no_upstream_files() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("control"), "Source: test\n").unwrap();

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_upstream_is_directory() {
        // debian/upstream as a directory must not be touched.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        let upstream_dir = debian.join("upstream");
        fs::create_dir(&upstream_dir).unwrap();
        fs::write(upstream_dir.join("metadata"), "Name: existing\n").unwrap();

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(upstream_dir.is_dir());
    }
}
