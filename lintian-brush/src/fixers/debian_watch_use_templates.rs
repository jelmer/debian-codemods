use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    // Parse using the unified parser
    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    // Only process v5 watch files
    if watch_file.version() != 5 {
        return Err(FixerError::NoChanges);
    }

    // Extract the v5 watch file
    let v5_file = match watch_file {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => wf,
        debian_watch::parse::ParsedWatchFile::LineBased(_) => {
            // Not a v5 file
            return Err(FixerError::NoChanges);
        }
    };

    let mut made_changes = false;
    let mut converted_templates = Vec::new();

    // Try to convert each entry to use templates
    for mut entry in v5_file.entries() {
        // Skip entries that already use templates
        if entry.as_deb822().get("Template").is_some() {
            continue;
        }

        if let Some(template) = entry.try_convert_to_template() {
            made_changes = true;
            // Extract template name for reporting
            let template_name = match template {
                debian_watch::templates::Template::GitHub { .. } => "GitHub",
                debian_watch::templates::Template::GitLab { .. } => "GitLab",
                debian_watch::templates::Template::PyPI { .. } => "PyPI",
                debian_watch::templates::Template::Npmregistry { .. } => "Npmregistry",
                debian_watch::templates::Template::Metacpan { .. } => "Metacpan",
            };
            converted_templates.push(template_name);
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the updated watch file
    fs::write(&watch_path, v5_file.to_string())?;

    // Create a descriptive message based on what was converted
    let description = if converted_templates.len() == 1 {
        format!(
            "Use {} template in watch file instead of explicit Source/Matching-Pattern.",
            converted_templates[0]
        )
    } else {
        "Use templates in watch file instead of explicit Source/Matching-Pattern.".to_string()
    };

    Ok(FixerResult::builder(description).build())
}

declare_fixer! {
    name: "debian-watch-use-templates",
    tags: [],
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
    fn test_convert_metacpan_to_template() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Example from the bug report
        let watch_content = r#"Version: 5

Source: https://cpan.metacpan.org/authors/id/
Matching-Pattern: .*/Mail-AuthenticationResults@ANY_VERSION@@ARCHIVE_EXT@
Searchmode: plain
"#;
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        // Should now use Metacpan template
        assert!(updated_content.contains("Template: Metacpan"));
        assert!(updated_content.contains("Dist: Mail-AuthenticationResults"));
        assert!(!updated_content.contains("Source: https://cpan.metacpan.org"));
        assert!(!updated_content.contains("Matching-Pattern:"));
        assert!(!updated_content.contains("Searchmode:"));
    }

    #[test]
    fn test_convert_github_to_template() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = r#"Version: 5

Source: https://github.com/torvalds/linux/tags
Matching-Pattern: .*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@
Searchmode: html
"#;
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        // Should now use GitHub template
        assert!(updated_content.contains("Template: GitHub"));
        assert!(updated_content.contains("Owner: torvalds"));
        assert!(updated_content.contains("Project: linux"));
        assert!(!updated_content.contains("Source: https://github.com"));
        assert!(!updated_content.contains("Matching-Pattern:"));
        assert!(!updated_content.contains("Searchmode:"));
    }

    #[test]
    fn test_convert_pypi_to_template() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = r#"Version: 5

Source: https://pypi.debian.net/bitbox02/
Matching-Pattern: https://pypi\.debian\.net/bitbox02/[^/]+\.tar\.gz#/.*-@ANY_VERSION@\.tar\.gz
Searchmode: plain
"#;
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        // Should now use PyPI template
        assert!(updated_content.contains("Template: PyPI"));
        assert!(updated_content.contains("Dist: bitbox02"));
        assert!(!updated_content.contains("Source: https://pypi.debian.net"));
        assert!(!updated_content.contains("Matching-Pattern:"));
        assert!(!updated_content.contains("Searchmode:"));
    }

    #[test]
    fn test_no_change_when_already_using_template() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Already using template
        let watch_content = r#"Version: 5

Template: Metacpan
Dist: Mail-AuthenticationResults
"#;
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_v5() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Version 4 watch file
        let watch_content =
            "version=4\nhttps://github.com/torvalds/linux/tags .*/v?([\\d.]+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_template_matches() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Custom source that doesn't match any template
        let watch_content = r#"Version: 5

Source: https://example.com/downloads/
Matching-Pattern: .*/v?(\d+\.\d+)\.tar\.gz
"#;
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
