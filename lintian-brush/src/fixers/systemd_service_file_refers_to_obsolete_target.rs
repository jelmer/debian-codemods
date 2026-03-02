use crate::{FixerError, FixerResult, LintianIssue};
use std::path::Path;
use std::str::FromStr;

const DEPRECATED_TARGETS: &[&str] = &["syslog.target"];

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
        if path.extension().is_none_or(|ext| ext != "service") {
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

        let mut file_changed = false;
        let mut removed_from_file: Vec<String> = Vec::new();

        for target in DEPRECATED_TARGETS {
            // Check if any After value contains this deprecated target
            let after_values = unit_section.get_all("After");
            let mut found = false;

            for after_value in &after_values {
                let targets: Vec<&str> = after_value.split_whitespace().collect();
                if targets.contains(target) {
                    found = true;
                    break;
                }
            }

            if found {
                // Get path relative to base_path for the issue
                let relative_path = path
                    .strip_prefix(base_path)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();

                // Create LintianIssue for this target
                let issue = LintianIssue::source_with_info(
                    "systemd-service-file-refers-to-obsolete-target",
                    vec![format!("{} {}", relative_path, target)],
                );

                if issue.should_fix(base_path) {
                    // Use remove_value to remove the target while preserving order
                    unit_section.remove_value("After", target);
                    file_changed = true;
                    removed_from_file.push(target.to_string());
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }

        if file_changed {
            // Write the modified content back
            std::fs::write(&path, unit.text())?;
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Remove references to obsolete targets in systemd unit files.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "systemd-service-file-refers-to-obsolete-target",
    tags: ["systemd-service-file-refers-to-obsolete-target"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    #[test]
    fn test_remove_syslog_target_from_after() {
        let input = r#"[Unit]
Description=Test Service
After=syslog.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let after_values = unit_section.get_all("After");
        assert_eq!(after_values, vec!["syslog.target"]);

        // Remove syslog.target
        unit_section.remove_value("After", "syslog.target");

        let expected = r#"[Unit]
Description=Test Service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_remove_syslog_target_from_multi_value() {
        let input = r#"[Unit]
Description=Test Service
After=network.target syslog.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let after_values = unit_section.get_all("After");
        assert_eq!(after_values, vec!["network.target syslog.target"]);

        // Remove syslog.target
        unit_section.remove_value("After", "syslog.target");

        let expected = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_no_syslog_target_unchanged() {
        let input = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let unit_section = unit.get_section("Unit").unwrap();

        let after_values = unit_section.get_all("After");
        assert_eq!(after_values, vec!["network.target"]);

        // Check that syslog.target is not present
        for after_value in &after_values {
            assert!(!after_value.split_whitespace().any(|t| t == "syslog.target"));
        }

        // No changes needed
        assert_eq!(unit.text(), input);
    }

    #[test]
    fn test_multiple_after_entries_with_syslog() {
        let input = r#"[Unit]
Description=Test Service
After=network.target
After=syslog.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let after_values = unit_section.get_all("After");
        assert_eq!(after_values, vec!["network.target", "syslog.target"]);

        // Remove syslog.target
        unit_section.remove_value("After", "syslog.target");

        let expected = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }
}
