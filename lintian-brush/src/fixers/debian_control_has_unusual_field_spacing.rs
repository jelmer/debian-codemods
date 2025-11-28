use crate::{declare_fixer, FixerError, FixerResult};
use deb822_lossless::Deb822;
use debian_analyzer::editor::check_generated_file;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn normalize_control_file(path: &Path) -> Result<bool, FixerError> {
    let content = fs::read_to_string(path)?;

    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => {
            return Err(FixerError::NoChanges);
        }
    };

    let original = deb822.to_string();

    for mut paragraph in deb822.paragraphs() {
        paragraph.normalize_field_spacing();
    }

    let normalized = deb822.to_string();

    if original == normalized {
        return Ok(false);
    }

    fs::write(path, normalized)?;
    Ok(true)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut made_changes = false;

    match check_generated_file(&control_path) {
        Err(generated_file) => {
            // Control file is generated, process template file
            if let Some(template_path) = &generated_file.template_path {
                if normalize_control_file(template_path)? {
                    made_changes = true;
                    // Also process the generated control file
                    normalize_control_file(&control_path)?;
                }
            } else {
                // No template path available, give up
                return Err(FixerError::NoChanges);
            }
        }
        Ok(()) => {
            // Control file is not generated, process it directly
            if normalize_control_file(&control_path)? {
                made_changes = true;
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Strip unusual field spacing from debian/control.")
            .fixed_tag("debian-control-has-unusual-field-spacing")
            .build(),
    )
}

declare_fixer! {
    name: "debian-control-has-unusual-field-spacing",
    tags: ["debian-control-has-unusual-field-spacing"],
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
    fn test_normalize_double_space() {
        let temp_dir = TempDir::new().unwrap();
        let control_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&control_dir).unwrap();

        let control_content = "Source: blah\nRecommends:  ${cdbs:Recommends}\n";
        let control_path = control_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Recommends: ${cdbs:Recommends}"));
        assert!(!updated_content.contains("Recommends:  "));
    }

    #[test]
    fn test_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let control_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&control_dir).unwrap();

        let control_content = "Source: blah\nRecommends: ${cdbs:Recommends}\n";
        let control_path = control_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_normalize_tab_after_colon() {
        let temp_dir = TempDir::new().unwrap();
        let control_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&control_dir).unwrap();

        let control_content = "Source: blah\nBuild-Depends:\tpython3\n";
        let control_path = control_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Build-Depends: python3"));
        assert!(!updated_content.contains("Build-Depends:\t"));
    }

    #[test]
    fn test_preserves_continuation_lines() {
        let temp_dir = TempDir::new().unwrap();
        let control_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&control_dir).unwrap();

        let control_content = "Source: blah\nBuild-Depends:  cdbs (>= 0.4.123~),\n  anotherline\n";
        let control_path = control_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Build-Depends: cdbs"));
        assert!(updated_content.contains("  anotherline"));
    }
}
