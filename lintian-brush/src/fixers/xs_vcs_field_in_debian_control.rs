use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Only process the source paragraph (XS-Vcs-* fields only appear there)
    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        // Find all fields that start with "XS-Vcs-" and collect their line numbers
        let xs_vcs_fields: Vec<(String, usize)> = paragraph
            .keys()
            .filter(|key| key.starts_with("XS-Vcs-"))
            .filter_map(|key| {
                paragraph
                    .get_entry(&key)
                    .map(|entry| (key.to_string(), entry.line() + 1))
            })
            .collect();

        for (xs_field, line_number) in xs_vcs_fields {
            let issue = LintianIssue::source_with_info(
                "adopted-extended-field",
                vec![format!(
                    "(in section for source) {} [debian/control:{}]",
                    xs_field, line_number
                )],
            );

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
            } else {
                let new_field = xs_field.strip_prefix("XS-").unwrap();
                paragraph.rename(&xs_field, new_field);
                fixed_issues.push(issue);
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
        FixerResult::builder("Remove unnecessary XS- prefix for Vcs- fields in debian/control")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "xs-vcs-field-in-debian-control",
    tags: ["adopted-extended-field"],
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
    fn test_xs_vcs_git_renamed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nXS-Vcs-Git: https://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary XS- prefix for Vcs- fields in debian/control"
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("XS-Vcs-Git"));
        assert!(content.contains("Vcs-Git: https://github.com/jelmer/lintian-brush"));
    }

    #[test]
    fn test_multiple_xs_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Vcs-Git: https://git.example.com/repo\nXS-Vcs-Browser: https://git.example.com/repo/browser\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove unnecessary XS- prefix for Vcs- fields in debian/control"
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let content = fs::read_to_string(&control_path).unwrap();
        assert!(!content.contains("XS-Vcs-Git"));
        assert!(!content.contains("XS-Vcs-Browser"));
        assert!(content.contains("Vcs-Git: https://git.example.com/repo"));
        assert!(content.contains("Vcs-Browser: https://git.example.com/repo/browser"));
    }

    #[test]
    fn test_no_xs_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nVcs-Git: https://git.example.com/repo\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
