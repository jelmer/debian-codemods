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
        if path.extension().and_then(|e| e.to_str()) != Some("service") {
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

        // Find the Service section
        let mut service_section = match unit.get_section("Service") {
            Some(section) => section,
            None => continue, // No Service section, skip this file
        };

        // Get the PIDFile value
        let old_pidfile = match service_section.get("PIDFile") {
            Some(pidfile) => pidfile,
            None => continue, // No PIDFile, skip this file
        };

        // Check if it contains /var/run/
        if !old_pidfile.contains("/var/run/") {
            continue; // No /var/run/, nothing to fix
        }

        // Get the relative path from base_path for the lintian issue
        let rel_path = path
            .strip_prefix(base_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        let issue = LintianIssue::source_with_info(
            "systemd-service-file-refers-to-var-run",
            vec![rel_path, "PIDFile".to_string(), old_pidfile.clone()],
        );

        if issue.should_fix(base_path) {
            // Replace /var/run/ with /run/ in the PIDFile
            let new_pidfile = old_pidfile.replace("/var/run/", "/run/");
            service_section.set("PIDFile", &new_pidfile);

            // Also replace the old PIDFile value in all other fields in the Service section
            for entry in service_section.entries() {
                if let (Some(key), Some(value)) = (entry.key(), entry.value()) {
                    if key == "PIDFile" {
                        continue; // Already handled
                    }

                    // Check if this field contains the old pidfile path
                    if value.contains(&old_pidfile) {
                        let new_value = value.replace(&old_pidfile, &new_pidfile);
                        service_section.set(&key, &new_value);
                    }
                }
            }

            // Write the modified content back
            std::fs::write(&path, unit.text())?;

            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Replace /var/run with /run for the Service PIDFile.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "systemd-service-file-pidfile-refers-to-var-run",
    tags: ["systemd-service-file-refers-to-var-run"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    #[test]
    fn test_replace_var_run_in_pidfile() {
        let input = r#"[Unit]
Description=Test Service

[Service]
Type=forking
PIDFile=/var/run/test.pid
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut service_section = unit.get_section("Service").unwrap();

        let old_pidfile = service_section.get("PIDFile").unwrap();
        assert_eq!(old_pidfile, "/var/run/test.pid");

        let new_pidfile = old_pidfile.replace("/var/run/", "/run/");
        service_section.set("PIDFile", &new_pidfile);

        let expected = r#"[Unit]
Description=Test Service

[Service]
Type=forking
PIDFile=/run/test.pid
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_replace_var_run_in_execstart_too() {
        let input = r#"[Unit]
Description=Test Service

[Service]
ExecStart=/sbin/daemon --pidfile=/var/run/test.pid
Type=forking
PIDFile=/var/run/test.pid
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let mut service_section = unit.get_section("Service").unwrap();

        let old_pidfile = service_section.get("PIDFile").unwrap();
        let new_pidfile = old_pidfile.replace("/var/run/", "/run/");

        service_section.set("PIDFile", &new_pidfile);

        // Also replace in other fields
        for entry in service_section.entries() {
            if let (Some(key), Some(value)) = (entry.key(), entry.value()) {
                if key == "PIDFile" {
                    continue;
                }
                if value.contains(&old_pidfile) {
                    let new_value = value.replace(&old_pidfile, &new_pidfile);
                    service_section.set(&key, &new_value);
                }
            }
        }

        let expected = r#"[Unit]
Description=Test Service

[Service]
ExecStart=/sbin/daemon --pidfile=/run/test.pid
Type=forking
PIDFile=/run/test.pid
"#;
        assert_eq!(unit.text(), expected);
    }

    #[test]
    fn test_no_var_run_unchanged() {
        let input = r#"[Unit]
Description=Test Service

[Service]
Type=forking
PIDFile=/run/test.pid
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let service_section = unit.get_section("Service").unwrap();

        let pidfile = service_section.get("PIDFile").unwrap();
        assert_eq!(pidfile, "/run/test.pid");
        assert!(!pidfile.contains("/var/run/"));

        // No changes needed
        assert_eq!(unit.text(), input);
    }

    #[test]
    fn test_no_pidfile() {
        let input = r#"[Unit]
Description=Test Service

[Service]
Type=simple
ExecStart=/sbin/daemon
"#;

        let unit = systemd_unit_edit::SystemdUnit::from_str(input).unwrap();
        let service_section = unit.get_section("Service").unwrap();

        let pidfile = service_section.get("PIDFile");
        assert_eq!(pidfile, None);

        // No changes needed
        assert_eq!(unit.text(), input);
    }
}
