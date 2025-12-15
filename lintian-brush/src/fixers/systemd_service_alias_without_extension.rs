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

        // Get the extension from the file path
        let required_ext = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => format!(".{}", ext),
            None => continue, // Skip files without extension
        };

        // Skip if not a .service file
        if required_ext != ".service" {
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

        // Get all "Alias" values
        let alias_values = unit_section.get_all("Alias");

        if !alias_values.is_empty() {
            let mut needs_fix = false;

            // Check each alias and see if it needs fixing
            for alias in &alias_values {
                let (_base, current_ext) = match alias.rfind('.') {
                    Some(idx) => (&alias[..idx], &alias[idx..]),
                    None => (alias.as_str(), ""),
                };

                if current_ext != required_ext {
                    needs_fix = true;
                    break;
                }
            }

            if needs_fix {
                // Get the relative path from base_path for the lintian issue
                let rel_path = path
                    .strip_prefix(base_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());

                let issue = LintianIssue::source_with_info(
                    "systemd-service-alias-without-extension",
                    vec![rel_path],
                );

                if issue.should_fix(base_path) {
                    // Remove all Alias entries
                    unit_section.remove_all("Alias");

                    // Add them back with the correct extension
                    for alias in alias_values {
                        let new_alias = if let Some(idx) = alias.rfind('.') {
                            let base = &alias[..idx];
                            format!("{}{}", base, required_ext)
                        } else {
                            format!("{}{}", alias, required_ext)
                        };

                        unit_section.add("Alias", &new_alias);
                    }

                    // Write the modified content back
                    std::fs::write(&path, unit.text())?;

                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
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
        FixerResult::builder("Use proper extensions in Alias in systemd files.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "systemd-service-alias-without-extension",
    tags: ["systemd-service-alias-without-extension"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    #[test]
    fn test_add_service_extension_to_alias() {
        let input = r#"[Unit]
Description=Test Service
Alias=bar

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let alias_values = unit_section.get_all("Alias");
        assert_eq!(alias_values, vec!["bar"]);

        // Fix the alias
        unit_section.remove_all("Alias");
        unit_section.add("Alias", "bar.service");

        let expected = r#"[Unit]
Description=Test Service
Alias=bar.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_replace_wrong_extension() {
        let input = r#"[Unit]
Description=Test Service
Alias=bar.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let alias_values = unit_section.get_all("Alias");
        assert_eq!(alias_values, vec!["bar.target"]);

        // Fix the alias - replace .target with .service
        unit_section.remove_all("Alias");
        let alias = &alias_values[0];
        let base = &alias[..alias.rfind('.').unwrap()];
        unit_section.add("Alias", &format!("{}.service", base));

        let expected = r#"[Unit]
Description=Test Service
Alias=bar.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_multiple_aliases() {
        let input = r#"[Unit]
Description=Test Service
Alias=foo
Alias=bar.target

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut unit_section = unit.get_section("Unit").unwrap();

        let alias_values = unit_section.get_all("Alias");
        assert_eq!(alias_values, vec!["foo", "bar.target"]);

        // Fix all aliases
        unit_section.remove_all("Alias");
        for alias in alias_values {
            let new_alias = if let Some(idx) = alias.rfind('.') {
                let base = &alias[..idx];
                format!("{}.service", base)
            } else {
                format!("{}.service", alias)
            };
            unit_section.add("Alias", &new_alias);
        }

        let expected = r#"[Unit]
Description=Test Service
Alias=foo.service
Alias=bar.service

[Service]
Type=oneshot
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_correct_extension_unchanged() {
        let input = r#"[Unit]
Description=Test Service
Alias=bar.service

[Service]
Type=oneshot
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let unit_section = unit.get_section("Unit").unwrap();

        let alias_values = unit_section.get_all("Alias");
        assert_eq!(alias_values, vec!["bar.service"]);

        // Check that it already has the correct extension
        for alias in &alias_values {
            assert!(alias.ends_with(".service"));
        }

        // No changes needed
        assert_eq!(unit.text(), input);
    }
}
