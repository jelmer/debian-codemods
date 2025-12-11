use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    let control_path = base_path.join("debian/control");

    // Check if debian/rules exists
    let rules_content = match fs::read(&rules_path) {
        Ok(content) => content,
        Err(_) => {
            // Unsure whether it actually needs cdbs
            return Err(FixerError::NoChanges);
        }
    };

    // Check if debian/rules uses cdbs
    let uses_cdbs = rules_content
        .windows(b"/usr/share/cdbs/".len())
        .any(|window| window == b"/usr/share/cdbs/");

    if uses_cdbs {
        // Still using cdbs, nothing to do
        return Err(FixerError::NoChanges);
    }

    // Not using cdbs, remove the dependency
    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut source = editor
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for field_name in ["Build-Depends", "Build-Depends-Indep"] {
        let paragraph = source.as_mut_deb822();
        if let Some(field_value) = paragraph.get(field_name) {
            use debian_control::lossless::relations::Relations;
            let (mut relations, _errors) = Relations::parse_relaxed(&field_value, true);

            if relations.drop_dependency("cdbs") {
                let issue = LintianIssue::source_with_info(
                    "unused-build-dependency-on-cdbs",
                    vec!["[debian/rules]".to_string()],
                );

                if issue.should_fix(base_path) {
                    if relations.is_empty() {
                        paragraph.remove(field_name);
                    } else {
                        paragraph.set(field_name, &relations.to_string());
                    }
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
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
        FixerResult::builder("Drop unused build-dependency on cdbs.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "unused-build-dependency-on-cdbs",
    tags: ["unused-build-dependency-on-cdbs"],
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
    fn test_removes_unused_cdbs() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debian/rules without cdbs
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        // Create debian/control with cdbs dependency
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!control_content.contains("cdbs"));
        assert!(control_content.contains("debhelper"));
    }

    #[test]
    fn test_keeps_cdbs_when_used() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debian/rules that uses cdbs
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\ninclude /usr/share/cdbs/1/rules/debhelper.mk\n",
        )
        .unwrap();

        // Create debian/control with cdbs dependency
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(control_content.contains("cdbs"));
    }

    #[test]
    fn test_no_rules_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        // Create debian/control with cdbs dependency
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nBuild-Depends: debhelper, cdbs\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
