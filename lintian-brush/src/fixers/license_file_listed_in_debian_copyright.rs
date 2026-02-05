use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use regex::Regex;
use std::path::Path;

pub fn run(
    basedir: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    use debian_copyright::lossless::Copyright;

    let copyright_path = basedir.join("debian/copyright");

    // Check minimum certainty
    let certainty = crate::Certainty::Certain;
    if !crate::certainty_sufficient(certainty, preferences.minimum_certainty) {
        return Err(FixerError::NotCertainEnough(
            certainty,
            preferences.minimum_certainty,
            vec![],
        ));
    }

    let content = std::fs::read_to_string(&copyright_path)?;
    let mut copyright: Copyright = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse copyright file: {:?}", e)))?;

    // Regex taken from /usr/share/lintian/checks/debian/copyright.pm
    let re_license = Regex::new(r"(^|/)(COPYING[^/]*|LICENSE)$").unwrap();

    let mut deleted = Vec::new();
    let mut overridden_issues = Vec::new();
    let mut patterns_to_remove = Vec::new();

    // Iterate through all files paragraphs
    for mut para in copyright.iter_files() {
        let files = para.files();
        let mut kept_files = Vec::new();

        for file_pattern in &files {
            if re_license.is_match(file_pattern) {
                let issue = crate::LintianIssue {
                    package: None,
                    package_type: Some(crate::PackageType::Source),
                    tag: Some("license-file-listed-in-debian-copyright".to_string()),
                    info: Some(format!("{} [debian/copyright]", file_pattern)),
                };

                if issue.should_fix(basedir) {
                    deleted.push(file_pattern.clone());
                } else {
                    overridden_issues.push(issue);
                    kept_files.push(file_pattern.as_str());
                }
            } else {
                kept_files.push(file_pattern.as_str());
            }
        }

        if kept_files.is_empty() {
            // Mark all files from this paragraph for removal
            // We'll use remove_files_by_pattern to remove the paragraph
            for pattern in &files {
                patterns_to_remove.push(pattern.clone());
            }
        } else if kept_files.len() != files.len() {
            // Update the paragraph with only kept files
            para.set_files(&kept_files);
        }
    }

    // Remove paragraphs that have no files left
    for pattern in &patterns_to_remove {
        copyright.remove_files_by_pattern(pattern);
    }

    if deleted.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the updated copyright file
    std::fs::write(&copyright_path, copyright.to_string())?;

    let deleted_str = deleted.join(", ");
    let mut result = FixerResult::builder(format!(
        "Remove listed license files ({}) from copyright.",
        deleted_str
    ))
    .certainty(certainty);

    // Add fixed tags for each deleted file
    for file in &deleted {
        result = result.fixed_issue(crate::LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            tag: Some("license-file-listed-in-debian-copyright".to_string()),
            info: Some(format!("{} [debian/copyright]", file)),
        });
    }

    Ok(result.build())
}

declare_fixer! {
    name: "license-file-listed-in-debian-copyright",
    tags: ["license-file-listed-in-debian-copyright"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}
