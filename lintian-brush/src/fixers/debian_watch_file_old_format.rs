use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use std::fs;
use std::path::Path;

const OBSOLETE_WATCH_FILE_FORMAT: u32 = 2;
const WATCH_FILE_LATEST_VERSION: u32 = 4;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let mut watch_file: debian_watch::WatchFile = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let version = watch_file.version();

    if version >= WATCH_FILE_LATEST_VERSION {
        return Err(FixerError::NoChanges);
    }

    // Determine the tag based on version
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

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Update the version
    watch_file.set_version(WATCH_FILE_LATEST_VERSION);

    // Write back the updated watch file
    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder(format!(
        "Update watch file format version to {}.",
        WATCH_FILE_LATEST_VERSION
    ))
    .fixed_issue(issue)
    .build())
}

declare_fixer! {
    name: "debian-watch-file-old-format",
    tags: ["older-debian-watch-file-standard", "obsolete-debian-watch-file-standard"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_update_old_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=3\nopts=pgpsigurlmangle=s/$/.asc/ https://example.com/foo foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.starts_with("version=4\n"));
        assert!(updated_content.contains("opts=pgpsigurlmangle"));
    }

    #[test]
    fn test_update_obsolete_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=2\nhttps://example.com/foo foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.starts_with("version=4\n"));
    }

    #[test]
    fn test_no_change_when_already_latest() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=4\nopts=\"pgpsigurlmangle=s/$/.asc/\" https://example.com/foo foo-(.*).tar.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
