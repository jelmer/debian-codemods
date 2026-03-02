use crate::{FixerError, FixerResult, LintianIssue};
use regex::Regex;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path, package: &str) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;

    // Create regex to match and remove the add-log-mailing-address line
    let re = Regex::new(r"add-log-mailing-address: .*\n").unwrap();

    let Some(mat) = re.find(&content) else {
        return Err(FixerError::NoChanges);
    };

    // Calculate line number (1-indexed)
    let line_number = content[..mat.start()].matches('\n').count() + 1;

    let issue = LintianIssue::source_with_info(
        "debian-changelog-file-contains-obsolete-user-emacs-settings",
        vec![format!(
            "[usr/share/doc/{}/changelog.Debian.gz:{}]",
            package, line_number
        )],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Remove the add-log-mailing-address line
    let new_content = re.replace_all(&content, "");

    if new_content == content {
        return Err(FixerError::NoChanges);
    }

    fs::write(&changelog_path, new_content.as_ref())?;

    Ok(FixerResult::builder(
        "Drop no longer supported add-log-mailing-address setting from debian/changelog.",
    )
    .fixed_issues(vec![issue])
    .certainty(crate::Certainty::Certain)
    .build())
}

declare_fixer! {
    name: "debian-changelog-file-contains-obsolete-user-emacs-settings",
    tags: ["debian-changelog-file-contains-obsolete-user-emacs-settings"],
    apply: |basedir, package, _version, _preferences| {
        run(basedir, package)
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

        let result = run(base_path, "libjcode-perl").unwrap();
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

        let result = run(base_path, "libjcode-perl");
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

        let result = run(base_path, "libjcode-perl");
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changelog_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path, "libjcode-perl");
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
