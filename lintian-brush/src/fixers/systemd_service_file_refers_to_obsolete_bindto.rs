use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::path::Path;
use std::str::FromStr;

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

        // Get all "BindTo" values
        let bindto_values = unit_section.get_all("BindTo");

        if !bindto_values.is_empty() {
            // Get the relative path from base_path for the lintian issue
            let rel_path = path
                .strip_prefix(base_path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());

            let issue = LintianIssue::source_with_info(
                "systemd-service-file-refers-to-obsolete-bindto",
                vec![rel_path],
            );

            if issue.should_fix(base_path) {
                // Remove all "BindTo" entries
                unit_section.remove_all("BindTo");

                // Add them back as "BindsTo"
                for value in bindto_values {
                    unit_section.add("BindsTo", &value);
                }

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
        FixerResult::builder("Rename BindTo key to BindsTo in systemd files.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "systemd-service-file-refers-to-obsolete-bindto",
    tags: ["systemd-service-file-refers-to-obsolete-bindto"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    #[test]
    fn test_rename_bindto_to_bindsto() {
        let input = r#"[Unit]
Description=Test Service
BindTo=foo.service

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let bindto_values = unit_section.get_all("BindTo");
        assert_eq!(bindto_values, vec!["foo.service"]);

        unit_section.remove_all("BindTo");
        for value in bindto_values {
            unit_section.add("BindsTo", &value);
        }

        let expected = r#"[Unit]
Description=Test Service
BindsTo=foo.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_rename_multiple_bindto_entries() {
        let input = r#"[Unit]
Description=Test Service
BindTo=foo.service
BindTo=bar.service

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let bindto_values = unit_section.get_all("BindTo");
        assert_eq!(bindto_values, vec!["foo.service", "bar.service"]);

        unit_section.remove_all("BindTo");
        for value in bindto_values {
            unit_section.add("BindsTo", &value);
        }

        let expected = r#"[Unit]
Description=Test Service
BindsTo=foo.service
BindsTo=bar.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_no_bindto_entries() {
        let input = r#"[Unit]
Description=Test Service
After=network.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let unit_section = unit.get_section("Unit").unwrap();

        let bindto_values = unit_section.get_all("BindTo");
        assert_eq!(bindto_values.len(), 0);
    }

    #[test]
    fn test_existing_bindsto_not_affected() {
        let input = r#"[Unit]
Description=Test Service
BindsTo=existing.service
BindTo=new.service

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let bindto_values = unit_section.get_all("BindTo");
        assert_eq!(bindto_values, vec!["new.service"]);

        let bindsto_before = unit_section.get_all("BindsTo");
        assert_eq!(bindsto_before, vec!["existing.service"]);

        unit_section.remove_all("BindTo");
        for value in bindto_values {
            unit_section.add("BindsTo", &value);
        }

        let expected = r#"[Unit]
Description=Test Service
BindsTo=existing.service
BindsTo=new.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }
}
