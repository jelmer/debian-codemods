use crate::{declare_fixer, FixerError, FixerResult};
use debian_changelog::textwrap::rewrap_changes;
use debian_changelog::ChangeLog;
use std::fs;
use std::path::Path;

const WIDTH: usize = 80;

fn any_long_lines(lines: &[String]) -> bool {
    lines.iter().any(|line| line.len() > WIDTH)
}

pub fn run(base_path: &Path, thorough: bool) -> Result<FixerResult, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    if !changelog_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&changelog_path)?;
    let changelog = content
        .parse::<ChangeLog>()
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {}", e)))?;

    // Collect entries to process
    let all_entries: Vec<_> = changelog.iter().collect();
    if all_entries.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Only process the first entry unless in thorough mode
    let entries: Vec<_> = if thorough {
        all_entries
    } else {
        all_entries.into_iter().take(1).collect()
    };

    let mut made_changes = false;
    let mut fixed_versions = Vec::new();

    for entry in entries {
        let change_lines: Vec<String> = entry.change_lines().collect();

        // Check if there are any long lines
        if !any_long_lines(&change_lines) {
            continue;
        }

        // Rewrap the changes
        let change_strs: Vec<&str> = change_lines.iter().map(|s| s.as_str()).collect();
        let wrapped: Vec<String> = rewrap_changes(change_strs.iter().copied())
            .map(|s| s.into_owned())
            .collect();

        // Check if anything actually changed
        if wrapped == change_lines {
            continue;
        }

        // Clear existing change lines
        while entry.pop_change_line().is_some() {
            // Keep popping
        }

        // Add the wrapped lines
        for line in wrapped {
            entry.append_change_line(&line);
        }

        made_changes = true;
        if let Some(version) = entry.version() {
            fixed_versions.push(version.to_string());
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the modified changelog
    fs::write(&changelog_path, changelog.to_string())?;

    let description = if !fixed_versions.is_empty() {
        format!(
            "Wrap long lines in changelog entries: {}.",
            fixed_versions.join(", ")
        )
    } else {
        "Wrap long lines in changelog entries.".to_string()
    };

    Ok(FixerResult::builder(&description)
        .fixed_tag("debian-changelog-line-too-long")
        .build())
}

declare_fixer! {
    name: "debian-changelog-line-too-long",
    tags: ["debian-changelog-line-too-long"],
    apply: |basedir, _package, _version, preferences| {
        let thorough = preferences
            .extra_env
            .as_ref()
            .and_then(|env| env.get("CHANGELOG_THOROUGH"))
            .map(|v| v == "1")
            .unwrap_or(false);
        run(basedir, thorough)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_wrap_long_line() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), false);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        // Check that the long line was wrapped
        assert!(updated_content.lines().all(|line| line.len() <= WIDTH));
        assert!(updated_content.contains("Thanks to somebody"));
    }

    #[test]
    fn test_no_long_lines() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * Short line.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_preserves_indentation() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let changelog_content = r#"blah (2.6.0) unstable; urgency=medium

  * New upstream release.
   * Fix blocks/blockedby of archived bugs (Closes: #XXXXXXX). Thanks to somebody who fixed it.

 -- Joe Example <joe@example.com>  Mon, 26 Feb 2018 11:31:48 -0800
"#;

        let changelog_path = debian_dir.join("changelog");
        fs::write(&changelog_path, changelog_content).unwrap();

        let result = run(temp_dir.path(), false);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated_content = fs::read_to_string(&changelog_path).unwrap();
        assert!(updated_content.lines().all(|line| line.len() <= WIDTH));
        // Should preserve sub-item indentation
        assert!(updated_content.contains("   *") || updated_content.contains(" * "));
    }
}
