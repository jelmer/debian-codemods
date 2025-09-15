use crate::{declare_fixer, FixerError, FixerResult};
use regex::Regex;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;

    // Create regex to match and remove the add-log-mailing-address line
    // The pattern looks for "Local variables:" followed by any content including
    // the add-log-mailing-address line, and replaces it with the same content
    // but without the add-log-mailing-address line
    let re =
        Regex::new(r"(?s)(Local variables:.*?)add-log-mailing-address: .*\n(.*?End:)").unwrap();

    if !re.is_match(&content) {
        return Err(FixerError::NoChanges);
    }

    let new_content = re.replace_all(&content, "$1$2");

    if new_content == content {
        return Err(FixerError::NoChanges);
    }

    fs::write(&changelog_path, new_content.as_ref())?;

    Ok(FixerResult::builder(
        "Drop no longer supported add-log-mailing-address setting from debian/changelog.",
    )
    .fixed_tags(vec![
        "debian-changelog-file-contains-obsolete-user-emacs-settings",
    ])
    .certainty(crate::Certainty::Certain)
    .build())
}

declare_fixer! {
    name: "debian-changelog-file-contains-obsolete-user-emacs-settings",
    tags: ["debian-changelog-file-contains-obsolete-user-emacs-settings"],
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
    fn test_remove_add_log_mailing_address() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nadd-log-mailing-address: \"joe@example.com\"\nEnd:\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Drop no longer supported add-log-mailing-address setting from debian/changelog."
        );
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let new_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(!new_content.contains("add-log-mailing-address"));
        assert!(new_content.contains("Local variables:"));
        assert!(new_content.contains("mode: debian-changelog"));
        assert!(new_content.contains("End:"));

        let expected = "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nEnd:\n";
        assert_eq!(new_content, expected);
    }

    #[test]
    fn test_no_add_log_mailing_address() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n\nLocal variables:\nmode: debian-changelog\nEnd:\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_local_variables() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let changelog_path = debian_dir.join("changelog");
        let content = "libjcode-perl (2.8-1) frozen unstable; urgency=low\n\n  * Upstream version.\n\n -- Blah <joe@example.com>  Thu, 15 Oct 1998 09:21:48 +0900\n";
        fs::write(&changelog_path, content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changelog_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
