use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&watch_path)?;

    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut made_changes = false;
    let mut fixed_issues = Vec::new();

    for mut entry in watch_file.entries() {
        let url = entry.url();

        // Check if URL uses githubredir.debian.net
        if !url.contains("githubredir.debian.net") {
            continue;
        }

        // Create issue with URL and line number
        let line_no = entry.line() + 1; // Convert to 1-indexed
        let matching = entry.matching_pattern().unwrap_or_default();
        let issue = LintianIssue::source_with_info(
            "debian-watch-file-uses-deprecated-githubredir",
            vec![format!("{} {} [debian/watch:{}]", url, matching, line_no)],
        );
        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        // Parse the URL to extract org and repo
        // URL format: http://githubredir.debian.net/github/ORG/REPO
        let url_parsed = url::Url::parse(&url).map_err(|_| FixerError::NoChanges)?;

        if url_parsed.host_str() != Some("githubredir.debian.net") {
            continue;
        }

        let path_parts: Vec<&str> = url_parsed.path().trim_matches('/').split('/').collect();

        if path_parts.len() < 3 || path_parts[0] != "github" {
            continue;
        }

        let org = path_parts[1];
        let repo = path_parts[2];

        // Update URL to use GitHub directly
        let new_url = format!("https://github.com/{}/{}/tags", org, repo);
        entry.set_url(&new_url);

        // Update matching pattern - extract just the filename part
        if let Some(pattern) = entry.matching_pattern() {
            if let Some(last_part) = pattern.rsplit('/').next() {
                let new_pattern = format!(".*/{}", last_part);
                entry.set_matching_pattern(&new_pattern);
            }
        }

        made_changes = true;
        fixed_issues.push(issue);
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder(
        "Remove use of githubredir - see https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html for details."
    )
    .fixed_issues(fixed_issues)
    .certainty(crate::Certainty::Confident)
    .build())
}

declare_fixer! {
    name: "debian-watch-file-uses-deprecated-githubredir",
    tags: ["debian-watch-file-uses-deprecated-githubredir"],
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
    fn test_replaces_githubredir() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content = "version=3\nhttp://githubredir.debian.net/github/developmentseed/mirror http://github.com/developmentseed/mirror/archive/(\\d+.*)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path()).unwrap();
        assert!(result.description.contains("githubredir"));

        let updated_content = fs::read_to_string(&watch_path).unwrap();
        assert!(updated_content.contains("https://github.com/developmentseed/mirror/tags"));
        assert!(!updated_content.contains("githubredir.debian.net"));
        assert!(updated_content.contains(".*/"));
    }

    #[test]
    fn test_no_watch_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_githubredir() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let watch_content =
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n";
        let watch_path = debian_dir.join("watch");
        fs::write(&watch_path, watch_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
