use crate::{Certainty, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor; // for reading control file and saving
use debian_control::lossless::Binary;
use std::path::Path;

fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian").join("control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    let dependency_fields = [
        "Depends",
        "Pre-Depends",
        "Recommends",
        "Suggests",
        "Enhances",
        "Breaks",
        "Conflicts",
    ];

    for mut binary in editor.binaries() {
        for dep_field in dependency_fields.iter() {
            if binary.as_deb822().contains_key(dep_field) {
                remove_circular_installation_prerequisite(
                    base_path,
                    &mut binary,
                    dep_field,
                    &mut fixed_issues,
                    &mut overridden_issues,
                );
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;
    Ok(
        FixerResult::builder("Remove circular dependency on self in package.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "circular-installation-prerequisite",
    tags: ["circular-installation-prerequisite"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

fn remove_circular_installation_prerequisite(
    base_path: &Path,
    binary: &mut Binary,
    field: &str,
    fixed_issues: &mut Vec<LintianIssue>,
    overridden_issues: &mut Vec<LintianIssue>,
) {
    // Create issue and check if we should fix it
    let issue = LintianIssue::source_with_info(
        "circular-installation-prerequisite",
        vec![field.to_string()],
    );

    if !issue.should_fix(base_path) {
        overridden_issues.push(issue);
        return;
    }

    let binary_name = binary.name().unwrap();
    match field {
        "Depends" => {
            if binary.depends().unwrap().has_relation(&binary_name) {
                if binary.depends().unwrap().len() == 1 {
                    binary.set_depends(None);
                } else {
                    let mut relations = binary.depends().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_depends(Some(&relations));
                }
            }
        }
        "Pre-Depends" => {
            if binary.pre_depends().unwrap().has_relation(&binary_name) {
                if binary.pre_depends().unwrap().len() == 1 {
                    binary.set_pre_depends(None);
                } else {
                    let mut relations = binary.pre_depends().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_pre_depends(Some(&relations));
                }
            }
        }
        "Recommends" => {
            if binary.recommends().unwrap().has_relation(&binary_name) {
                if binary.recommends().unwrap().len() == 1 {
                    binary.set_recommends(None);
                } else {
                    let mut relations = binary.recommends().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_recommends(Some(&relations));
                }
            }
        }
        "Suggests" => {
            if binary.suggests().unwrap().has_relation(&binary_name) {
                if binary.suggests().unwrap().len() == 1 {
                    binary.set_suggests(None);
                } else {
                    let mut relations = binary.suggests().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_suggests(Some(&relations));
                }
            }
        }
        "Enhances" => {
            if binary.enhances().unwrap().has_relation(&binary_name) {
                if binary.enhances().unwrap().len() == 1 {
                    binary.set_enhances(None);
                } else {
                    let mut relations = binary.enhances().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_enhances(Some(&relations));
                }
            }
        }
        "Breaks" => {
            if binary.breaks().unwrap().has_relation(&binary_name) {
                if binary.breaks().unwrap().len() == 1 {
                    binary.set_breaks(None);
                } else {
                    let mut relations = binary.breaks().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_breaks(Some(&relations));
                }
            }
        }
        "Conflicts" => {
            if binary.conflicts().unwrap().has_relation(&binary_name) {
                if binary.conflicts().unwrap().len() == 1 {
                    binary.set_conflicts(None);
                } else {
                    let mut relations = binary.conflicts().unwrap();
                    relations.drop_dependency(&binary_name);
                    binary.set_conflicts(Some(&relations));
                }
            }
        }
        _ => {
            todo!()
        }
    }

    fixed_issues.push(issue);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_does_not_have_dependency() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: build-essential, debhelper-compat (= 13)

Package: test-package
Architecture: any
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify the change was made
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(control_content, updated_content);
    }
}
