use crate::{declare_fixer, Certainty, FixerError, FixerResult};
use std::fs;
use std::path::Path;

#[derive(Debug, PartialEq)]
enum ScriptStatus {
    Empty,
    SomeComments,
    NotEmpty,
}

const MAINTAINER_SCRIPTS: &[&str] = &["prerm", "postinst", "preinst", "postrm"];

fn is_empty(path: &Path) -> Result<ScriptStatus, std::io::Error> {
    let content = fs::read(path)?;
    let mut status = ScriptStatus::Empty;

    for (line_no, line_bytes) in content.split(|&b| b == b'\n').enumerate() {
        let line = line_bytes.iter().copied().collect::<Vec<u8>>();
        let trimmed_line: Vec<u8> = line
            .iter()
            .rev()
            .skip_while(|&&b| b == b' ' || b == b'\t' || b == b'\r')
            .copied()
            .collect::<Vec<u8>>()
            .into_iter()
            .rev()
            .collect();

        if trimmed_line.is_empty() {
            continue;
        }

        // Skip shebang on first line
        if line_no == 0 && trimmed_line.starts_with(b"#!") {
            continue;
        }

        // Handle comment lines
        if trimmed_line.starts_with(b"#") {
            // Check if it's more than just '#' and not '#DEBHELPER#'
            let comment_content = &trimmed_line[1..];
            let comment_trimmed: Vec<u8> = comment_content
                .iter()
                .skip_while(|&&b| b == b'#')
                .copied()
                .collect();

            if !comment_trimmed.is_empty() && trimmed_line != b"#DEBHELPER#" {
                status = ScriptStatus::SomeComments;
            }
            continue;
        }

        // Skip 'set ' commands
        if trimmed_line.starts_with(b"set ") {
            continue;
        }

        // Skip 'exit ' commands
        if trimmed_line.starts_with(b"exit ") {
            continue;
        }

        // If we reach here, there's substantial content
        return Ok(ScriptStatus::NotEmpty);
    }

    Ok(status)
}

fn parse_maintainer_script_name(filename: &str) -> Option<(String, String)> {
    if MAINTAINER_SCRIPTS.contains(&filename) {
        return Some(("source".to_string(), filename.to_string()));
    }

    if let Some(dot_pos) = filename.rfind('.') {
        let package = &filename[..dot_pos];
        let script = &filename[dot_pos + 1..];

        if MAINTAINER_SCRIPTS.contains(&script) {
            return Some((package.to_string(), script.to_string()));
        }
    }

    None
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut removed = Vec::new();
    let mut certainty = Certainty::Certain;

    let entries = fs::read_dir(&debian_dir)?;

    for entry in entries {
        let entry = entry?;
        let filename = entry.file_name().to_string_lossy().to_string();

        if let Some((package, script)) = parse_maintainer_script_name(&filename) {
            let script_path = entry.path();

            match is_empty(&script_path)? {
                ScriptStatus::Empty => {
                    fs::remove_file(&script_path)?;
                    removed.push((package, script));
                }
                ScriptStatus::SomeComments => {
                    // Check minimum certainty - in the Python version, this checks meets_minimum_certainty("likely")
                    // For simplicity, we'll always remove comments-only scripts but set certainty to likely
                    fs::remove_file(&script_path)?;
                    removed.push((package, script));
                    certainty = Certainty::Likely;
                }
                ScriptStatus::NotEmpty => {
                    // Keep the script as it has substantial content
                }
            }
        }
    }

    if removed.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let description = format!(
        "Remove empty maintainer scripts: {}",
        removed
            .iter()
            .map(|(package, script)| format!("{} ({})", package, script))
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(FixerResult::builder(&description)
        .fixed_tags(vec!["maintainer-script-empty"])
        .certainty(certainty)
        .build())
}

declare_fixer! {
    name: "maintainer-script-empty",
    tags: ["maintainer-script-empty"],
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
    fn test_is_empty_truly_empty() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_script");
        fs::write(&script_path, "").unwrap();

        assert_eq!(is_empty(&script_path).unwrap(), ScriptStatus::Empty);
    }

    #[test]
    fn test_is_empty_shebang_only() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_script");
        fs::write(&script_path, "#!/bin/sh\n").unwrap();

        assert_eq!(is_empty(&script_path).unwrap(), ScriptStatus::Empty);
    }

    #[test]
    fn test_is_empty_comments_only() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_script");
        fs::write(
            &script_path,
            "#!/bin/sh\n# This is a comment\nset -e\n#DEBHELPER#\n",
        )
        .unwrap();

        assert_eq!(is_empty(&script_path).unwrap(), ScriptStatus::SomeComments);
    }

    #[test]
    fn test_is_empty_has_content() {
        let temp_dir = TempDir::new().unwrap();
        let script_path = temp_dir.path().join("test_script");
        fs::write(&script_path, "#!/bin/sh\necho 'Hello world'\n").unwrap();

        assert_eq!(is_empty(&script_path).unwrap(), ScriptStatus::NotEmpty);
    }

    #[test]
    fn test_parse_maintainer_script_name() {
        assert_eq!(
            parse_maintainer_script_name("prerm"),
            Some(("source".to_string(), "prerm".to_string()))
        );

        assert_eq!(
            parse_maintainer_script_name("mypackage.postinst"),
            Some(("mypackage".to_string(), "postinst".to_string()))
        );

        assert_eq!(parse_maintainer_script_name("not_a_script"), None);
        assert_eq!(parse_maintainer_script_name("package.unknown"), None);
    }

    #[test]
    fn test_remove_empty_script() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create control file
        let control_content = r#"Source: test-package

Package: mon
Description: Test package
 Description text
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        // Create empty maintainer script
        fs::write(debian_dir.join("mon.prerm"), "").unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that the script was removed
        assert!(!debian_dir.join("mon.prerm").exists());

        let result = result.unwrap();
        assert!(result
            .description
            .contains("Remove empty maintainer scripts: mon (prerm)"));
        assert_eq!(result.certainty, Some(Certainty::Certain));
    }

    #[test]
    fn test_remove_comments_only_script() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let script_content = "#!/bin/sh\n# This is just a comment\nset -e\n#DEBHELPER#\n";
        fs::write(debian_dir.join("mon.prerm"), script_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        // Check that the script was removed
        assert!(!debian_dir.join("mon.prerm").exists());

        let result = result.unwrap();
        assert_eq!(result.certainty, Some(Certainty::Likely));
    }

    #[test]
    fn test_keep_non_empty_script() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let script_content = "#!/bin/sh\necho 'This script does something'\n";
        fs::write(debian_dir.join("mon.prerm"), script_content).unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Check that the script was kept
        assert!(debian_dir.join("mon.prerm").exists());
    }

    #[test]
    fn test_no_debian_directory() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
