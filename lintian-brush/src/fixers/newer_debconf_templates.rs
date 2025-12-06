use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Command;

fn read_hashes(po_dir: &Path) -> Result<HashMap<String, String>, FixerError> {
    let mut hashes = HashMap::new();

    for entry in fs::read_dir(po_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let contents = fs::read(&path)?;
            let hash = format!("{:x}", Sha1::digest(&contents));
            hashes.insert(path.to_string_lossy().to_string(), hash);
        }
    }

    Ok(hashes)
}

fn update_timestamp(path: &Path, timestamp: i64) -> Result<(), FixerError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();

    for line in reader.lines() {
        let line = line?;

        if line.starts_with("\"POT-Creation-Date: ") {
            // Format timestamp as "YYYY-MM-DD HH:MM+0000"
            use chrono::{DateTime, Utc};
            let dt = DateTime::from_timestamp(timestamp, 0).unwrap();
            lines.push(format!(
                "\"POT-Creation-Date: {}\\n\"",
                dt.format("%Y-%m-%d %H:%M+0000")
            ));
        } else {
            lines.push(line);
        }
    }

    let mut file = fs::File::create(path)?;
    for line in &lines {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

fn debconf_updatepo(base_path: &Path) -> Result<(), FixerError> {
    let output = Command::new("debconf-updatepo")
        .current_dir(base_path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FixerError::NoChanges
            } else {
                FixerError::from(e)
            }
        })?;

    if !output.status.success() {
        return Err(FixerError::Other(format!(
            "debconf-updatepo failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let po_dir = base_path.join("debian/po");

    if !po_dir.is_dir() {
        return Err(FixerError::NoChanges);
    }

    // Check if we should update timestamps
    if let Ok(timestamp_str) = std::env::var("DEBCONF_GETTEXTIZE_TIMESTAMP") {
        let timestamp: i64 = timestamp_str
            .parse()
            .map_err(|_| FixerError::Other("Invalid DEBCONF_GETTEXTIZE_TIMESTAMP".to_string()))?;

        let old_hashes = read_hashes(&po_dir)?;
        debconf_updatepo(base_path)?;
        let new_hashes = read_hashes(&po_dir)?;

        for (path_str, old_hash) in &old_hashes {
            if let Some(new_hash) = new_hashes.get(path_str) {
                if old_hash != new_hash {
                    update_timestamp(Path::new(path_str), timestamp)?;
                }
            }
        }
    } else {
        debconf_updatepo(base_path)?;
    }

    Ok(
        FixerResult::builder("Run debconf-updatepo after template changes.".to_string())
            .fixed_tag("newer-debconf-templates")
            .build(),
    )
}

declare_fixer! {
    name: "newer-debconf-templates",
    tags: ["newer-debconf-templates"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_read_hashes() {
        let temp_dir = TempDir::new().unwrap();
        let po_dir = temp_dir.path();

        // Create some test files
        fs::write(po_dir.join("file1.po"), b"content1").unwrap();
        fs::write(po_dir.join("file2.po"), b"content2").unwrap();

        let hashes = read_hashes(po_dir).unwrap();

        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains_key(&po_dir.join("file1.po").to_string_lossy().to_string()));
        assert!(hashes.contains_key(&po_dir.join("file2.po").to_string_lossy().to_string()));

        // Verify hashes are consistent
        let hashes2 = read_hashes(po_dir).unwrap();
        assert_eq!(hashes, hashes2);

        // Change a file and verify hash changes
        fs::write(po_dir.join("file1.po"), b"different content").unwrap();
        let hashes3 = read_hashes(po_dir).unwrap();
        assert_ne!(
            hashes.get(&po_dir.join("file1.po").to_string_lossy().to_string()),
            hashes3.get(&po_dir.join("file1.po").to_string_lossy().to_string())
        );
    }

    #[test]
    fn test_update_timestamp() {
        let temp_dir = TempDir::new().unwrap();
        let po_file = temp_dir.path().join("test.po");

        // Create a PO file with a POT-Creation-Date line
        let content = r#"# Translation file
"POT-Creation-Date: 2020-01-01 12:00+0000\n"
"Some other line\n"
"#;
        fs::write(&po_file, content).unwrap();

        // Update with timestamp for 2023-06-15 15:16:40 UTC
        let timestamp = 1686842200; // 2023-06-15 15:16:40 UTC
        update_timestamp(&po_file, timestamp).unwrap();

        let updated_content = fs::read_to_string(&po_file).unwrap();
        assert!(updated_content.contains("\"POT-Creation-Date: 2023-06-15 15:16+0000\\n\""));
        assert!(updated_content.contains("\"Some other line\\n\""));
    }

    #[test]
    fn test_update_timestamp_no_pot_creation_date() {
        let temp_dir = TempDir::new().unwrap();
        let po_file = temp_dir.path().join("test.po");

        // Create a PO file without POT-Creation-Date
        let content = r#"# Translation file
"Some line\n"
"Another line\n"
"#;
        fs::write(&po_file, content).unwrap();

        let timestamp = 1686842200;
        update_timestamp(&po_file, timestamp).unwrap();

        // Content should remain unchanged
        let updated_content = fs::read_to_string(&po_file).unwrap();
        assert!(updated_content.contains("\"Some line\\n\""));
        assert!(!updated_content.contains("POT-Creation-Date"));
    }

    #[test]
    fn test_read_hashes_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let hashes = read_hashes(temp_dir.path()).unwrap();
        assert_eq!(hashes.len(), 0);
    }

    #[test]
    fn test_read_hashes_with_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let po_dir = temp_dir.path();

        // Create a file and a subdirectory
        fs::write(po_dir.join("file.po"), b"content").unwrap();
        fs::create_dir(po_dir.join("subdir")).unwrap();
        fs::write(po_dir.join("subdir/nested.po"), b"nested").unwrap();

        let hashes = read_hashes(po_dir).unwrap();

        // Should only include the file in the directory, not subdirectories
        assert_eq!(hashes.len(), 1);
        assert!(hashes.contains_key(&po_dir.join("file.po").to_string_lossy().to_string()));
    }
}
