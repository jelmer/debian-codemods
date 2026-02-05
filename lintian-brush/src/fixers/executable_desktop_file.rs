use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// Remove executable bit from desktop files
declare_fixer! {
    name: "executable-desktop-file",
    tags: ["executable-desktop-file"],
    apply: |basedir, _package, _version, _preferences| {
        let debian_dir = basedir.join("debian");

        if !debian_dir.exists() {
            return Err(FixerError::NoChanges);
        }

        let mut fixed_issues = Vec::new();
        let mut overridden_issues = Vec::new();

        // Find all .desktop files in debian/ directory
        let desktop_files = find_desktop_files(&debian_dir)?;

        if desktop_files.is_empty() {
            return Err(FixerError::NoChanges);
        }

        // Remove executable bit from each desktop file
        for desktop_file in desktop_files {
            let metadata = fs::metadata(&desktop_file)?;
            let current_mode = metadata.permissions().mode();

            // Check if file is executable
            if (current_mode & 0o111) != 0 {
                // Get filename for installed path
                let filename = desktop_file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let installed_path = format!("usr/share/applications/{}", filename);
                let perms_octal = format!("{:04o}", current_mode & 0o777);

                let issue = LintianIssue::source_with_info(
                    "executable-desktop-file",
                    vec![format!("{} [{}]", perms_octal, installed_path)],
                );

                if issue.should_fix(basedir) {
                    if remove_executable_bit(&desktop_file)? {
                        fixed_issues.push(issue);
                    }
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        if fixed_issues.is_empty() {
            if !overridden_issues.is_empty() {
                return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
            }
            return Err(FixerError::NoChanges);
        }

        Ok(FixerResult::builder("Remove executable bit from desktop files.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build())
    }
}

fn find_desktop_files(debian_dir: &Path) -> Result<Vec<std::path::PathBuf>, FixerError> {
    let mut desktop_files = Vec::new();

    let entries = fs::read_dir(debian_dir)
        .map_err(|e| FixerError::Other(format!("Failed to read debian directory: {}", e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| FixerError::Other(format!("Failed to read directory entry: {}", e)))?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("desktop") {
            desktop_files.push(path);
        }
    }

    Ok(desktop_files)
}

fn remove_executable_bit(file_path: &Path) -> Result<bool, FixerError> {
    let metadata = fs::metadata(file_path).map_err(|e| {
        FixerError::Other(format!(
            "Failed to get metadata for {}: {}",
            file_path.display(),
            e
        ))
    })?;

    let current_perms = metadata.permissions();
    let current_mode = current_perms.mode();

    // Remove executable bits (user, group, other)
    let new_mode = current_mode & !0o111;

    // Check if permissions actually need to be changed
    if current_mode == new_mode {
        return Ok(false); // No change needed
    }

    let new_perms = fs::Permissions::from_mode(new_mode);
    fs::set_permissions(file_path, new_perms).map_err(|e| {
        FixerError::Other(format!(
            "Failed to set permissions for {}: {}",
            file_path.display(),
            e
        ))
    })?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn test_find_desktop_files() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create some test files
        fs::write(debian_dir.join("test.desktop"), "test content").unwrap();
        fs::write(debian_dir.join("another.desktop"), "test content").unwrap();
        fs::write(debian_dir.join("not-desktop.txt"), "test content").unwrap();

        let desktop_files = find_desktop_files(&debian_dir).unwrap();

        assert_eq!(desktop_files.len(), 2);
        assert!(desktop_files
            .iter()
            .any(|p| p.file_name().unwrap() == "test.desktop"));
        assert!(desktop_files
            .iter()
            .any(|p| p.file_name().unwrap() == "another.desktop"));
    }

    #[test]
    fn test_find_desktop_files_empty_directory() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_files = find_desktop_files(&debian_dir).unwrap();
        assert_eq!(desktop_files.len(), 0);
    }

    #[test]
    fn test_find_desktop_files_nonexistent_directory() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("nonexistent");

        let result = find_desktop_files(&debian_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_executable_bit() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.desktop");

        // Create file with executable permissions (755)
        fs::write(&file_path, "test content").unwrap();
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&file_path, perms).unwrap();

        // Remove executable bit
        let changed = remove_executable_bit(&file_path).unwrap();
        assert!(changed);

        // Check new permissions (should be 644)
        let metadata = fs::metadata(&file_path).unwrap();
        let new_mode = metadata.permissions().mode() & 0o777;
        assert_eq!(new_mode, 0o644);
    }

    #[test]
    fn test_remove_executable_bit_already_not_executable() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.desktop");

        // Create file without executable permissions (644)
        fs::write(&file_path, "test content").unwrap();
        let perms = fs::Permissions::from_mode(0o644);
        fs::set_permissions(&file_path, perms).unwrap();

        // Try to remove executable bit
        let changed = remove_executable_bit(&file_path).unwrap();
        assert!(!changed); // Should return false since no change was needed
    }

    #[test]
    fn test_remove_executable_bit_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("nonexistent.desktop");

        let result = remove_executable_bit(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_executable_desktop_file_fixer_with_executable_files() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create executable desktop files
        let desktop1 = debian_dir.join("app1.desktop");
        let desktop2 = debian_dir.join("app2.desktop");

        fs::write(&desktop1, "[Desktop Entry]\nName=App1").unwrap();
        fs::write(&desktop2, "[Desktop Entry]\nName=App2").unwrap();

        // Make them executable
        let exec_perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&desktop1, exec_perms.clone()).unwrap();
        fs::set_permissions(&desktop2, exec_perms).unwrap();

        // Test the conversion logic indirectly
        let desktop_files = find_desktop_files(&debian_dir).unwrap();
        assert_eq!(desktop_files.len(), 2);

        let changed1 = remove_executable_bit(&desktop1).unwrap();
        let changed2 = remove_executable_bit(&desktop2).unwrap();

        assert!(changed1);
        assert!(changed2);

        // Verify permissions were changed
        let meta1 = fs::metadata(&desktop1).unwrap();
        let meta2 = fs::metadata(&desktop2).unwrap();

        assert_eq!(meta1.permissions().mode() & 0o777, 0o644);
        assert_eq!(meta2.permissions().mode() & 0o777, 0o644);
    }

    #[test]
    fn test_executable_desktop_file_fixer_no_desktop_files() {
        let temp_dir = tempdir().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create non-desktop files
        fs::write(debian_dir.join("control"), "Source: test").unwrap();
        fs::write(debian_dir.join("changelog"), "test changelog").unwrap();

        let desktop_files = find_desktop_files(&debian_dir).unwrap();
        assert_eq!(desktop_files.len(), 0);
    }
}
