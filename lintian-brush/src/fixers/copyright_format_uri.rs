use crate::{FixerError, FixerResult, LintianIssue};
use regex::bytes::Regex;
use std::fs;
use std::path::Path;

const CORRECT_FORMAT: &[u8] =
    b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read(&copyright_path)?;

    if content.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Find the end of first line
    let first_line_end = content
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(content.len());
    let first_line = &content[..first_line_end + 1.min(content.len() - first_line_end)];

    // Check for insecure debian.org copyright format URI
    let insecure_regex = Regex::new(
        r"^(Format|Format-Specification): (http://www\.debian\.org/doc/packaging-manuals/copyright-format/1\.0.*)\n"
    ).unwrap();

    // Check for wiki copyright format URI
    let wiki_regex = Regex::new(
        r"^(Format|Format-Specification): (http://wiki\.debian\.org/Proposals/CopyrightFormat.*)\n",
    )
    .unwrap();

    let (is_wiki, old_uri) = if let Some(caps) = insecure_regex.captures(first_line) {
        let uri = String::from_utf8_lossy(&caps[2]).to_string();
        (false, uri)
    } else if let Some(caps) = wiki_regex.captures(first_line) {
        let uri = String::from_utf8_lossy(&caps[2]).to_string();
        (true, uri)
    } else {
        return Err(FixerError::NoChanges);
    };

    // Only replace if it's different from what we want
    if first_line == CORRECT_FORMAT {
        return Err(FixerError::NoChanges);
    }

    // Create issues and check which should be fixed
    let insecure_issue =
        LintianIssue::source_with_info("insecure-copyright-format-uri", vec![old_uri.clone()]);

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    if insecure_issue.should_fix(base_path) {
        fixed_issues.push(insecure_issue);
    } else {
        overridden_issues.push(insecure_issue);
    }

    if is_wiki {
        let wiki_issue =
            LintianIssue::source_with_info("wiki-copyright-format-uri", vec![old_uri.clone()]);

        if wiki_issue.should_fix(base_path) {
            fixed_issues.push(wiki_issue);
        } else {
            overridden_issues.push(wiki_issue);
        }
    }

    if fixed_issues.is_empty() {
        return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
    }

    // Build new content with replaced first line
    let mut new_content = Vec::new();
    new_content.extend_from_slice(CORRECT_FORMAT);
    if first_line_end + 1 < content.len() {
        new_content.extend_from_slice(&content[first_line_end + 1..]);
    }

    fs::write(&copyright_path, &new_content)?;

    Ok(
        FixerResult::builder("Use secure copyright file specification URI.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "copyright-format-uri",
    tags: ["insecure-copyright-format-uri", "wiki-copyright-format-uri"],
    // Must convert http to https before adding version (unversioned-copyright-format-uri)
    before: ["unversioned-copyright-format-uri"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_insecure_uri() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n";
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("insecure-copyright-format-uri".to_string())
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/".to_string())
        );

        let updated_content = fs::read(&copyright_path).unwrap();
        let updated_str = String::from_utf8_lossy(&updated_content);
        assert!(updated_str.starts_with("Format: https://www.debian.org"));
        assert!(updated_str.contains("Upstream-Name: test"));
    }

    #[test]
    fn test_wiki_uri() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: http://wiki.debian.org/Proposals/CopyrightFormat\nUpstream-Name: test\n";
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        let updated_content = fs::read(&copyright_path).unwrap();
        let updated_str = String::from_utf8_lossy(&updated_content);
        assert!(updated_str.starts_with("Format: https://www.debian.org"));
        assert!(updated_str.contains("Upstream-Name: test"));
    }

    #[test]
    fn test_already_secure() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let copyright_content =
            b"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n";
        let copyright_path = debian_dir.join("copyright");
        fs::write(&copyright_path, copyright_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "test", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
