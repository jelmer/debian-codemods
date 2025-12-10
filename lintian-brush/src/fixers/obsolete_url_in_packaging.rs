use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

// Include the generated obsolete sites definitions
include!(concat!(env!("OUT_DIR"), "/obsolete_sites.rs"));

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    if let Some(mut source) = editor.source() {
        let paragraph = source.as_mut_deb822();

        if let Some(homepage) = paragraph.get("Homepage") {
            let homepage_str = homepage.to_string();

            // Parse the URL to get the hostname
            if let Ok(url) = url::Url::parse(&homepage_str) {
                if let Some(hostname) = url.host_str() {
                    if is_obsolete_site(hostname) {
                        let issue = LintianIssue::source_with_info(
                            "obsolete-url-in-packaging",
                            vec![format!("{} [debian/control]", homepage_str.trim())],
                        );

                        if !issue.should_fix(base_path) {
                            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
                        }

                        paragraph.remove("Homepage");
                        editor.commit()?;

                        return Ok(FixerResult::builder(
                            "Drop fields with obsolete URLs.".to_string(),
                        )
                        .fixed_issue(issue)
                        .build());
                    }
                }
            }
        }
    }

    Err(FixerError::NoChanges)
}

declare_fixer! {
    name: "obsolete-url-in-packaging",
    tags: ["obsolete-url-in-packaging"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_remove_obsolete_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomepage: http://foo.tigris.org/\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.description, "Drop fields with obsolete URLs.");

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated_content, "Source: blah\n");
    }

    #[test]
    fn test_no_homepage_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: blah\n").unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_obsolete_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomepage: https://www.example.com/\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
