use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::path::Path;
use std::str::FromStr;

/// Check if a space-separated list contains a specific item
fn list_contains(value: &str, item: &str) -> bool {
    value.split_whitespace().any(|v| v == item)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Find all systemd service files
    let debian_path = base_path.join("debian");
    if !debian_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for entry in std::fs::read_dir(&debian_path)? {
        let entry = entry?;
        let path = entry.path();

        // Skip if not a .service file
        if !path.extension().map_or(false, |ext| ext == "service") {
            continue;
        }

        // Skip symbolic links
        if path.is_symlink() {
            continue;
        }

        // Read the service file
        let content = std::fs::read_to_string(&path)?;

        // Parse using systemd-unit-edit
        let unit = systemd_unit_edit::SystemdUnit::from_str(&content).map_err(|e| {
            FixerError::Other(format!("Failed to parse {}: {:?}", path.display(), e))
        })?;

        // Find the Unit section
        let mut unit_section = match unit.get_section("Unit") {
            Some(section) => section,
            None => continue, // No Unit section, skip this file
        };

        // Check conditions
        let default_deps = unit_section.get("DefaultDependencies");
        let conflicts = unit_section.get("Conflicts");
        let before_values = unit_section.get_all("Before");

        let should_add = default_deps.as_deref() == Some("no")
            && conflicts
                .as_ref()
                .map_or(false, |c| list_contains(c, "shutdown.target"))
            && !before_values
                .iter()
                .any(|b| list_contains(b, "shutdown.target"));

        if should_add {
            // Get the relative path from base_path for the lintian issue
            let rel_path = path
                .strip_prefix(base_path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());

            let issue = LintianIssue::source_with_info(
                "systemd-service-file-shutdown-problems",
                vec![rel_path],
            );

            if issue.should_fix(base_path) {
                // Add a new "Before=shutdown.target" entry
                unit_section.add("Before", "shutdown.target");

                // Write the modified content back
                std::fs::write(&path, unit.text())?;

                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Add Before=shutdown.target to Unit section.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "systemd-service-file-shutdown-problems",
    tags: ["systemd-service-file-shutdown-problems"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_contains() {
        assert!(list_contains(
            "ssh.service shutdown.target",
            "shutdown.target"
        ));
        assert!(list_contains("shutdown.target", "shutdown.target"));
        assert!(!list_contains("ssh.service", "shutdown.target"));
        assert!(!list_contains("", "shutdown.target"));
    }
}
