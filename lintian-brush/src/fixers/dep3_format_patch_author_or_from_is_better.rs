use crate::{declare_fixer, Certainty, FixerError, FixerResult};
use dep3::lossless::PatchHeader;
use patchkit::quilt::{Series, SeriesEntry};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let patches_path = base_path.join("debian/patches");

    if !patches_path.exists() || !patches_path.is_dir() {
        return Err(FixerError::NoChanges);
    }

    let series_path = patches_path.join("series");
    if !series_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let series_file = fs::File::open(&series_path)?;
    let series = Series::read(series_file)
        .map_err(|e| FixerError::Other(format!("Failed to read series file: {}", e)))?;

    if series.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut made_changes = false;

    for entry in &series.entries {
        if let SeriesEntry::Patch { name, .. } = entry {
            let patch_path = patches_path.join(name);

            let content = match fs::read_to_string(&patch_path) {
                Ok(content) => content,
                Err(_) => continue, // Skip if patch file doesn't exist
            };

            // Find where the diff starts
            let mut header_end = content.len();
            for (i, line) in content.lines().enumerate() {
                if line.starts_with("---")
                    || line.starts_with("diff ")
                    || line.starts_with("Index:")
                {
                    // Count bytes up to this line
                    header_end = content.lines().take(i).map(|l| l.len() + 1).sum();
                    break;
                }
            }

            let header_str = &content[..header_end];
            let body = &content[header_end..];

            let mut header = match header_str.parse::<PatchHeader>() {
                Ok(h) => h,
                Err(_) => continue, // Skip if we can't parse the header
            };

            // Check if there's an Origin field with an email address
            if let Some((_category, origin)) = header.origin() {
                let origin_str = origin.to_string();
                if origin_str.contains('@') {
                    // Set it as Author instead
                    header.set_author(&origin_str);

                    // Remove the Origin field
                    header.as_deb822_mut().remove("Origin");

                    made_changes = true;

                    // Reconstruct the patch file
                    let new_content = format!("{}{}", header, body);

                    fs::write(&patch_path, new_content)?;
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Use Author instead of Origin in patch headers.")
            .fixed_tag("dep3-format-patch-author-or-from-is-better")
            .certainty(Certainty::Confident)
            .build(),
    )
}

declare_fixer! {
    name: "dep3-format-patch-author-or-from-is-better",
    tags: ["dep3-format-patch-author-or-from-is-better"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_replace_origin_with_author() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let series_content = "fix-typo.patch\n";
        fs::write(patches_dir.join("series"), series_content).unwrap();

        let patch_content = r#"Description: Fix a typo
Origin: john@example.com
Bug: https://example.com/bugs/123

--- a/file.txt
+++ b/file.txt
@@ -1 +1 @@
-teh
+the
"#;
        fs::write(patches_dir.join("fix-typo.patch"), patch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Error: {:?}", result);

        let updated_content = fs::read_to_string(patches_dir.join("fix-typo.patch")).unwrap();
        assert!(updated_content.contains("Author: john@example.com"));
        assert!(!updated_content.contains("Origin:"));
    }

    #[test]
    fn test_no_changes_when_origin_without_email() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let series_content = "fix-typo.patch\n";
        fs::write(patches_dir.join("series"), series_content).unwrap();

        let patch_content = r#"Description: Fix a typo
Origin: upstream

--- a/file.txt
+++ b/file.txt
"#;
        fs::write(patches_dir.join("fix-typo.patch"), patch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_origin() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let series_content = "fix-typo.patch\n";
        fs::write(patches_dir.join("series"), series_content).unwrap();

        let patch_content = r#"Description: Fix a typo
Author: jane@example.com

--- a/file.txt
+++ b/file.txt
"#;
        fs::write(patches_dir.join("fix-typo.patch"), patch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_patches() {
        let temp_dir = TempDir::new().unwrap();
        let patches_dir = temp_dir.path().join("debian/patches");
        fs::create_dir_all(&patches_dir).unwrap();

        let series_content = "patch1.patch\npatch2.patch\n";
        fs::write(patches_dir.join("series"), series_content).unwrap();

        let patch1_content = r#"Description: Patch 1
Origin: user1@example.com

--- a/file1.txt
+++ b/file1.txt
"#;
        fs::write(patches_dir.join("patch1.patch"), patch1_content).unwrap();

        let patch2_content = r#"Description: Patch 2
Origin: user2@example.com

--- a/file2.txt
+++ b/file2.txt
"#;
        fs::write(patches_dir.join("patch2.patch"), patch2_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        let updated1 = fs::read_to_string(patches_dir.join("patch1.patch")).unwrap();
        assert!(updated1.contains("Author: user1@example.com"));
        assert!(!updated1.contains("Origin:"));

        let updated2 = fs::read_to_string(patches_dir.join("patch2.patch")).unwrap();
        assert!(updated2.contains("Author: user2@example.com"));
        assert!(!updated2.contains("Origin:"));
    }
}
