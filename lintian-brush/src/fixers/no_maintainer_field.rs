use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_changelog::get_maintainer_from_env;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    // Check if Maintainer field already exists
    if let Some(source) = editor.source() {
        if source.as_deb822().contains_key("Maintainer") {
            return Err(FixerError::NoChanges);
        }
    } else {
        return Err(FixerError::NoChanges);
    }

    // Create issue and check if we should fix it
    let issue = LintianIssue::source_with_info(
        "required-field",
        vec!["debian/control Maintainer".to_string()],
    );
    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Get maintainer from environment using debian_changelog
    // The builtin_fixers.rs wrapper will have already set environment variables from preferences.extra_env
    // So we can just read from the actual environment here
    let (fullname, email) =
        get_maintainer_from_env(|s| std::env::var(s).ok()).ok_or_else(|| {
            FixerError::Other("Could not determine maintainer from environment".to_string())
        })?;

    let maintainer_value = format!("{} <{}>", fullname, email);

    // Set the Maintainer field
    if let Some(mut source) = editor.source() {
        source.as_mut_deb822().set("Maintainer", &maintainer_value);
    }

    editor.commit()?;

    Ok(FixerResult::builder(format!(
        "Set the maintainer field to: {} <{}>.",
        fullname, email
    ))
    .fixed_issues(vec![issue])
    .certainty(crate::Certainty::Possible)
    .build())
}

declare_fixer! {
    name: "no-maintainer-field",
    tags: ["required-field"],
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
    fn test_maintainer_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nMaintainer: Existing User <existing@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();

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
