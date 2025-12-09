use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use regex::Regex;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let issue = LintianIssue::source_with_info("old-fsf-address-in-copyright-file", vec![]);

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    let content = fs::read_to_string(&copyright_path)?;

    // Create regex to match the old FSF address and replace with new one
    // The pattern needs to handle whitespace preservation like the perl script
    let old_fsf_regex = Regex::new(
        r"(?s)([ ]+)Free Software Foundation, Inc\., 59 Temple Place - Suite 330,\s*\n([ ]+)Boston, MA 02111-1307, USA\."
    ).unwrap();

    if !old_fsf_regex.is_match(&content) {
        return Err(FixerError::NoChanges);
    }

    let new_content = old_fsf_regex.replace_all(
        &content,
        "${1}Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n${2}MA 02110-1301, USA."
    );

    if new_content == content {
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, new_content.as_ref())?;

    Ok(FixerResult::builder("Update FSF postal address")
        .fixed_issue(issue)
        .certainty(crate::Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "old-fsf-address-in-copyright-file",
    tags: ["old-fsf-address-in-copyright-file"],
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
    fn test_update_fsf_address() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        let content = "This program is free software...\n  Free Software Foundation, Inc., 59 Temple Place - Suite 330,\n  Boston, MA 02111-1307, USA.\nOn Debian systems...";
        fs::write(&copyright_path, content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Update FSF postal address");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let new_content = fs::read_to_string(&copyright_path).unwrap();
        assert!(!new_content.contains("59 Temple Place"));
        assert!(!new_content.contains("02111-1307"));
        assert!(new_content.contains("51 Franklin St, Fifth Floor"));
        assert!(new_content.contains("02110-1301"));

        let expected = "This program is free software...\n  Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n  MA 02110-1301, USA.\nOn Debian systems...";
        assert_eq!(new_content, expected);
    }

    #[test]
    fn test_no_old_fsf_address() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        let content = "This program is free software...\n  Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n  MA 02110-1301, USA.\nOn Debian systems...";
        fs::write(&copyright_path, content).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_different_whitespace() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        let content = "License text:\n    Free Software Foundation, Inc., 59 Temple Place - Suite 330,\n    Boston, MA 02111-1307, USA.\nMore text.";
        fs::write(&copyright_path, content).unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        let new_content = fs::read_to_string(&copyright_path).unwrap();
        assert!(new_content.contains("    Free Software Foundation, Inc., 51 Franklin St, Fifth Floor, Boston,\n    MA 02110-1301, USA."));
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
