use crate::{FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use debian_analyzer::editor::check_generated_file;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn normalize_control_file(
    path: &Path,
    base_path: &Path,
) -> Result<(bool, Vec<LintianIssue>, Vec<LintianIssue>), FixerError> {
    let content = fs::read_to_string(path)?;

    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => {
            return Err(FixerError::NoChanges);
        }
    };

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut made_changes = false;

    for paragraph in deb822.paragraphs() {
        // Check each entry before normalizing to create issues for fields with unusual spacing
        for mut entry in paragraph.entries() {
            if let Some(key) = entry.key() {
                let line_number = entry.line() + 1;

                // normalize_field_spacing returns true if it made changes
                if entry.normalize_field_spacing() {
                    let issue = LintianIssue::source_with_info(
                        "debian-control-has-unusual-field-spacing",
                        vec![format!("{} [debian/control:{}]", key, line_number)],
                    );

                    if issue.should_fix(base_path) {
                        fixed_issues.push(issue);
                        made_changes = true;
                    } else {
                        overridden_issues.push(issue);
                    }
                }
            }
        }
    }

    if made_changes {
        fs::write(path, deb822.to_string())?;
    }

    Ok((made_changes, fixed_issues, overridden_issues))
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut all_fixed_issues = Vec::new();
    let mut all_overridden_issues = Vec::new();
    let mut made_changes = false;

    match check_generated_file(&control_path) {
        Err(generated_file) => {
            // Control file is generated, process template file
            if let Some(template_path) = &generated_file.template_path {
                let (changed, fixed, overridden) =
                    normalize_control_file(template_path, base_path)?;
                if changed {
                    made_changes = true;
                    all_fixed_issues.extend(fixed);
                    all_overridden_issues.extend(overridden);
                    // Also process the generated control file
                    normalize_control_file(&control_path, base_path)?;
                }
            } else {
                // No template path available, give up
                return Err(FixerError::NoChanges);
            }
        }
        Ok(()) => {
            // Control file is not generated, process it directly
            let (changed, fixed, overridden) = normalize_control_file(&control_path, base_path)?;
            if changed {
                made_changes = true;
                all_fixed_issues.extend(fixed);
                all_overridden_issues.extend(overridden);
            }
        }
    }

    if !made_changes {
        if !all_overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(all_overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Strip unusual field spacing from debian/control.")
            .fixed_issues(all_fixed_issues)
            .overridden_issues(all_overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "debian-control-has-unusual-field-spacing",
    tags: ["debian-control-has-unusual-field-spacing"],
    // Must normalize field spacing before whitespace cleanup to avoid conflicts
    before: ["file-contains-trailing-whitespace"],
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
