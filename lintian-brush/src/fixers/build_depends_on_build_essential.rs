use crate::{declare_fixer, Certainty, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Paragraph;
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::Relations;
use std::path::Path;

fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian").join("control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if let Some(mut source) = editor.source() {
        // Handle Build-Depends
        filter_build_essential_from_field(
            base_path,
            source.as_mut_deb822(),
            "Build-Depends",
            &mut fixed_issues,
            &mut overridden_issues,
        );
        // Handle Build-Depends-Indep
        filter_build_essential_from_field(
            base_path,
            source.as_mut_deb822(),
            "Build-Depends-Indep",
            &mut fixed_issues,
            &mut overridden_issues,
        );
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;
    Ok(
        FixerResult::builder("Drop unnecessary dependency on build-essential.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "build-depends-on-build-essential",
    tags: ["build-depends-on-build-essential"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

fn filter_build_essential_from_field(
    base_path: &Path,
    para: &mut Paragraph,
    field: &str,
    fixed_issues: &mut Vec<LintianIssue>,
    overridden_issues: &mut Vec<LintianIssue>,
) {
    let old_contents = para.get(field).unwrap_or_default();
    if old_contents.is_empty() {
        return;
    }

    let mut relations: Relations = match old_contents.parse() {
        Ok(r) => r,
        Err(_) => return,
    };

    // Check if build-essential is present and whether we should fix it
    let mut has_build_essential = false;
    for entry in relations.entries() {
        for relation in entry.relations() {
            if relation.name() == "build-essential" {
                has_build_essential = true;
                break;
            }
        }
        if has_build_essential {
            break;
        }
    }

    if !has_build_essential {
        return;
    }

    // Create issue and check if we should fix it
    let issue =
        LintianIssue::source_with_info("build-depends-on-build-essential", vec![field.to_string()]);

    if !issue.should_fix(base_path) {
        overridden_issues.push(issue);
        return;
    }

    // Remove build-essential relations from each entry
    for entry in relations.entries() {
        let mut to_remove = Vec::new();

        for (i, relation) in entry.relations().enumerate() {
            if relation.name() == "build-essential" {
                to_remove.push(i);
            }
        }

        // Remove relations in reverse order to avoid index shifts
        for i in to_remove.into_iter().rev() {
            entry.remove_relation(i);
        }
    }

    // Remove empty entries
    let mut empty_entries = Vec::new();
    for (i, entry) in relations.entries().enumerate() {
        if entry.relations().count() == 0 {
            empty_entries.push(i);
        }
    }

    for i in empty_entries.into_iter().rev() {
        relations.remove_entry(i);
    }

    let new_contents = relations.to_string();
    if new_contents.trim().is_empty() || relations.is_empty() {
        para.remove(field);
    } else {
        para.set(field, &new_contents);
    }

    fixed_issues.push(issue);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_removes_build_essential_from_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = r#"Source: test-package
Build-Depends: build-essential, debhelper-compat (= 13)

Package: test-package
Architecture: any
Depends: ${shlibs:Depends}, ${misc:Depends}
"#;

        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        // Verify the change was made
        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("build-essential"));
        assert!(updated_content.contains("debhelper-compat (= 13)"));
    }
}
