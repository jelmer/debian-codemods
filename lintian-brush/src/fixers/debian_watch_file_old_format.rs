use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, LintianIssue, PackageType};
use std::path::{Path, PathBuf};

const OBSOLETE_WATCH_FILE_FORMAT: u32 = 2;
const WATCH_FILE_LATEST_VERSION: u32 = 5;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_rel = PathBuf::from("debian/watch");
    let watch_abs = base_path.join(&watch_rel);
    if !watch_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&watch_abs)?;
    let watch_file = match debian_watch::parse::parse(&content) {
        Ok(w) => w,
        Err(_) => return Ok(Vec::new()),
    };
    let version = watch_file.version();
    if version >= WATCH_FILE_LATEST_VERSION {
        return Ok(Vec::new());
    }

    // Convert to v5 (deb822 form). The conversion is a wholesale rewrite
    // of the file's structure, so we emit it as a plain Write.
    let v5_file = match watch_file {
        debian_watch::parse::ParsedWatchFile::LineBased(ref wf) => {
            match debian_watch::convert_to_v5(wf) {
                Ok(v5) => v5,
                Err(_) => return Ok(Vec::new()),
            }
        }
        debian_watch::parse::ParsedWatchFile::Deb822(_) => return Ok(Vec::new()),
    };

    let tag = if version <= OBSOLETE_WATCH_FILE_FORMAT {
        "obsolete-debian-watch-file-standard"
    } else {
        "older-debian-watch-file-standard"
    };
    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some(tag.to_string()),
        info: Some(version.to_string()),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!(
            "Update watch file format version to {}.",
            WATCH_FILE_LATEST_VERSION
        ),
        vec![Action::Filesystem(FilesystemAction::Write {
            file: watch_rel,
            content: v5_file.to_string().into_bytes(),
        })],
    )
    .with_certainty(Certainty::Confident)])
}

declare_fixer! {
    name: "debian-watch-file-old-format",
    tags: ["older-debian-watch-file-standard", "obsolete-debian-watch-file-standard"],
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
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_update_old_watch_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=3\nopts=pgpsigurlmangle=s/$/.asc/ https://example.com/foo foo-(.*).tar.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\nPgpsigurlmangle: s/$/.asc/\n",
        );
    }

    #[test]
    fn test_update_obsolete_watch_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=2\nhttps://example.com/foo foo-(.*).tar.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\n",
        );
    }

    #[test]
    fn test_no_change_when_already_v5() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
