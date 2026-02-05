use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");
    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = std::fs::read_to_string(&copyright_path)?;

    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Err(FixerError::NoChanges),
    };

    let mut modified = false;
    let mut updated_licenses = HashSet::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();
    let pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9-.]+)").unwrap();

    for mut para in deb822.paragraphs() {
        let Some(license_field) = para.get("License") else {
            continue;
        };

        if license_field.is_empty() {
            continue;
        }

        // Extract the synopsis (first line) from the license field
        let lines: Vec<&str> = license_field.lines().collect();
        let synopsis = lines.first().unwrap_or(&"").trim();

        if synopsis.is_empty() {
            continue;
        }

        let new_text = pattern.replace_all(&license_field, |caps: &regex::Captures| {
            let path_str = &caps[0];
            let license_name = &caps[1];

            // Check if this is a symlink path that should be replaced
            if let Some(replacement) = replace_symlink_path(synopsis, path_str, license_name) {
                // Create issues for this symlink path
                let path_without_slash = path_str.trim_start_matches('/');
                let symlink_issue = LintianIssue::source_with_info(
                    "copyright-refers-to-symlink-license",
                    vec![path_without_slash.to_string()],
                );
                let versionless_issue = LintianIssue::source_with_info(
                    "copyright-refers-to-versionless-license-file",
                    vec![path_without_slash.to_string()],
                );

                let symlink_should_fix = symlink_issue.should_fix(base_path);
                let versionless_should_fix = versionless_issue.should_fix(base_path);

                // Track which issues are fixed vs overridden
                if symlink_should_fix {
                    fixed_issues.push(symlink_issue);
                } else {
                    overridden_issues.push(symlink_issue);
                }

                if versionless_should_fix {
                    fixed_issues.push(versionless_issue);
                } else {
                    overridden_issues.push(versionless_issue);
                }

                // Make the replacement if at least one issue should be fixed
                if symlink_should_fix || versionless_should_fix {
                    updated_licenses.insert(synopsis.to_string());
                    replacement
                } else {
                    // Both issues are overridden, don't replace
                    path_str.to_string()
                }
            } else {
                path_str.to_string()
            }
        });

        if new_text != license_field {
            para.set("License", &new_text);
            modified = true;
        }
    }

    if !modified {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the updated copyright file
    std::fs::write(&copyright_path, deb822.to_string())?;

    let updated_list: Vec<_> = updated_licenses.into_iter().collect();
    Ok(FixerResult::builder(format!(
        "Refer to specific version of license {}.",
        updated_list.join(", ")
    ))
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

fn replace_symlink_path(synopsis: &str, path: &str, _license_name: &str) -> Option<String> {
    // Strip the "+" from the synopsis to get the base version
    let base_synopsis = synopsis.trim_end_matches('+');

    // Check if the path is a symlink
    let path_obj = std::path::Path::new(path);
    let was_link = path_obj.read_link().is_ok();

    // Create the new path with the versioned license
    let newpath = format!("/usr/share/common-licenses/{}", base_synopsis);
    let newpath_obj = std::path::Path::new(&newpath);

    // Check if the new path exists and is not a symlink
    if !newpath_obj.exists() || newpath_obj.read_link().is_ok() {
        return None;
    }

    // Check if newpath starts with oldpath + "-"
    // e.g., "/usr/share/common-licenses/GPL-3" starts with "/usr/share/common-licenses/GPL" + "-"
    if !newpath.starts_with(&format!("{}-", path)) {
        return None;
    }

    // Only replace if it was a symlink or we're making it more specific
    if was_link || newpath != path {
        Some(newpath)
    } else {
        None
    }
}

declare_fixer! {
    name: "copyright-refers-to-symlink-license",
    tags: ["copyright-refers-to-symlink-license", "copyright-refers-to-versionless-license-file"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_path_regex() {
        let pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9-.]+)").unwrap();

        // Test matching
        assert!(pattern.is_match("/usr/share/common-licenses/GPL"));
        assert!(pattern.is_match("/usr/share/common-licenses/GPL-3"));
        assert!(pattern.is_match("/usr/share/common-licenses/LGPL-2.1"));
        assert!(pattern.is_match("text /usr/share/common-licenses/Apache more text"));

        // Test capturing
        let caps = pattern
            .captures("/usr/share/common-licenses/GPL-3")
            .unwrap();
        assert_eq!(&caps[1], "GPL-3");

        let caps = pattern.captures("/usr/share/common-licenses/LGPL").unwrap();
        assert_eq!(&caps[1], "LGPL");
    }

    #[test]
    fn test_synopsis_extraction() {
        // Test single line
        let license_field = "GPL-3+";
        let lines: Vec<&str> = license_field.lines().collect();
        let synopsis = lines.first().unwrap_or(&"").trim();
        assert_eq!(synopsis, "GPL-3+");

        // Test multiline
        let license_field = "GPL-3+\n Some license text\n More text";
        let lines: Vec<&str> = license_field.lines().collect();
        let synopsis = lines.first().unwrap_or(&"").trim();
        assert_eq!(synopsis, "GPL-3+");

        // Test with leading/trailing whitespace
        let license_field = "  Apache-2.0  \n License text";
        let lines: Vec<&str> = license_field.lines().collect();
        let synopsis = lines.first().unwrap_or(&"").trim();
        assert_eq!(synopsis, "Apache-2.0");
    }

    #[test]
    fn test_synopsis_trimming() {
        // Test that we strip "+" from synopsis for path construction
        assert_eq!("GPL-3".trim_end_matches('+'), "GPL-3");
        assert_eq!("GPL-3+".trim_end_matches('+'), "GPL-3");
        assert_eq!("Apache-2.0+".trim_end_matches('+'), "Apache-2.0");
        assert_eq!("LGPL-2.1".trim_end_matches('+'), "LGPL-2.1");
    }

    #[test]
    fn test_newpath_construction() {
        let synopsis = "GPL-3+";
        let base_synopsis = synopsis.trim_end_matches('+');
        let newpath = format!("/usr/share/common-licenses/{}", base_synopsis);
        assert_eq!(newpath, "/usr/share/common-licenses/GPL-3");

        let synopsis = "LGPL-2.1";
        let base_synopsis = synopsis.trim_end_matches('+');
        let newpath = format!("/usr/share/common-licenses/{}", base_synopsis);
        assert_eq!(newpath, "/usr/share/common-licenses/LGPL-2.1");
    }

    #[test]
    fn test_path_prefix_check() {
        // Test the prefix logic
        let old_path = "/usr/share/common-licenses/GPL";
        let new_path = "/usr/share/common-licenses/GPL-3";
        assert!(new_path.starts_with(&format!("{}-", old_path)));

        let old_path = "/usr/share/common-licenses/LGPL";
        let new_path = "/usr/share/common-licenses/LGPL-2.1";
        assert!(new_path.starts_with(&format!("{}-", old_path)));

        // Negative cases
        let old_path = "/usr/share/common-licenses/Apache";
        let new_path = "/usr/share/common-licenses/GPL-3";
        assert!(!new_path.starts_with(&format!("{}-", old_path)));
    }

    #[test]
    fn test_regex_replacement() {
        let pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9-.]+)").unwrap();
        let text = "See /usr/share/common-licenses/GPL for details.";

        let result = pattern.replace_all(text, "/usr/share/common-licenses/GPL-3");
        assert_eq!(result, "See /usr/share/common-licenses/GPL-3 for details.");

        let text =
            "Multiple refs: /usr/share/common-licenses/GPL and /usr/share/common-licenses/LGPL";
        let result =
            pattern.replace_all(text, |caps: &regex::Captures| format!("{}-NEW", &caps[0]));
        assert_eq!(
            result,
            "Multiple refs: /usr/share/common-licenses/GPL-NEW and /usr/share/common-licenses/LGPL-NEW"
        );
    }
}
