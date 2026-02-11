use crate::{declare_fixer, FixerError, FixerResult, LintianIssue, PackageType};
use std::fs;
use std::path::Path;

const OBSOLETE_WATCH_FILE_FORMAT: u32 = 2;
const WATCH_FILE_LATEST_VERSION: u32 = 5;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    // Parse using the unified parser
    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let version = watch_file.version();

    // Already version 5, no changes needed
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

    // Convert to version 5 - we need to extract the linebased file to convert it
    let v5_file = match watch_file {
        debian_watch::parse::ParsedWatchFile::LineBased(ref wf) => debian_watch::convert_to_v5(wf)
            .map_err(|e| FixerError::Other(format!("Failed to convert to v5: {}", e)))?,
        debian_watch::parse::ParsedWatchFile::Deb822(_) => {
            // Already v5, shouldn't reach here due to version check above
            return Err(FixerError::NoChanges);
        }
    };

    // Write back the updated watch file
    fs::write(&watch_path, v5_file.to_string())?;

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
        // Should now be in version 5 deb822 format
        let expected = "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\nPGP-Signature-URL-Mangle: s/$/.asc/\n";
        assert_eq!(updated_content, expected);
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
        // Should now be in version 5 deb822 format
        let expected =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\n";
        assert_eq!(updated_content, expected);
    }

    #[test]
    fn test_no_change_when_already_v5() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Version 5 deb822 format
        let watch_content =
            "Version: 5\n\nSource: https://example.com/foo\nMatching-Pattern: foo-(.*).tar.gz\n";
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
