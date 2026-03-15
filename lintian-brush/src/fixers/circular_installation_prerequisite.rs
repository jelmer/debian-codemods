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

    let Some(binary_name) = binary.name() else {
        return
    };

    match field {
        "Depends" => {
            let mut depends = binary.depends().unwrap();
            if depends.has_relation(&binary_name) {
                if depends.len() == 1 {
                    binary.set_depends(None);
                } else {
                    depends.drop_dependency(&binary_name);
                    binary.set_depends(Some(&depends));
                }
            }
        }
        "Pre-Depends" => {
            let mut pre_depends = binary.pre_depends().unwrap();
            if pre_depends.has_relation(&binary_name) {
                if pre_depends.len() == 1 {
                    binary.set_pre_depends(None);
                } else {
                    pre_depends.drop_dependency(&binary_name);
                    binary.set_pre_depends(Some(&pre_depends));
                }
            }
        }
        "Recommends" => {
            let mut recommends = binary.recommends().unwrap();
            if recommends.has_relation(&binary_name) {
                if recommends.len() == 1 {
                    binary.set_recommends(None);
                } else {
                    recommends.drop_dependency(&binary_name);
                    binary.set_recommends(Some(&recommends));
                }
            }
        }
        "Suggests" => {
            let mut suggests = binary.suggests().unwrap();
            if suggests.has_relation(&binary_name) {
                if suggests.len() == 1 {
                    binary.set_suggests(None);
                } else {
                    suggests.drop_dependency(&binary_name);
                    binary.set_suggests(Some(&suggests));
                }
            }
        }
        "Enhances" => {
            let mut enhances = binary.enhances().unwrap();
            if enhances.has_relation(&binary_name) {
                if enhances.len() == 1 {
                    binary.set_enhances(None);
                } else {
                    enhances.drop_dependency(&binary_name);
                    binary.set_enhances(Some(&enhances));
                }
            }
        }
        "Breaks" => {
            let mut breaks = binary.breaks().unwrap();
            if breaks.has_relation(&binary_name) {
                if breaks.len() == 1 {
                    binary.set_breaks(None);
                } else {
                    breaks.drop_dependency(&binary_name);
                    binary.set_breaks(Some(&breaks));
                }
            }
        }
        "Conflicts" => {
            let mut conflicts = binary.conflicts().unwrap();
            if conflicts.has_relation(&binary_name) {
                if conflicts.len() == 1 {
                    binary.set_conflicts(None);
                } else {
                    conflicts.drop_dependency(&binary_name);
                    binary.set_conflicts(Some(&conflicts));
                }
            }
        }
        _ => {
            unreachable!()
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
