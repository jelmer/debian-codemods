use crate::{FixerError, FixerResult, LintianIssue};
use debian_control::lossless::Control;
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn read_debhelper_compat_file(path: &Path) -> Result<Option<u32>, std::io::Error> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            match trimmed.parse::<u32>() {
                Ok(version) => Ok(Some(version)),
                Err(_) => Ok(None),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn get_debhelper_compat_level_from_control(control: &Control) -> Result<Option<u32>, FixerError> {
    let source = control.source().ok_or(FixerError::NoChanges)?;

    // Get Build-Depends
    let build_depends = match source.build_depends() {
        Some(bd) => bd,
        None => return Ok(None),
    };

    // Look for debhelper-compat (= N) in Build-Depends
    for entry in build_depends.entries() {
        for relation in entry.relations() {
            if relation.name() == "debhelper-compat" {
                // Check for version constraint
                if let Some((constraint, version)) = relation.version() {
                    // constraint is like "=" and version is the actual version
                    if constraint.to_string() == "=" {
                        if let Ok(compat_level) = version.to_string().parse::<u32>() {
                            return Ok(Some(compat_level));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn update_rules_file(
    rules_path: &Path,
    compat_version: Option<u32>,
    compat_source: &str,
    base_path: &Path,
) -> Result<(bool, Option<LintianIssue>), FixerError> {
    let content = fs::read_to_string(rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // First, extract DH_COMPAT value as integer and line number
    let dh_compat_info: Option<(u32, usize)> =
        makefile.find_variable("DH_COMPAT").next().and_then(|def| {
            let value = def.raw_value()?.trim().parse::<u32>().ok()?;
            let line = def.line() + 1;
            Some((value, line))
        });

    if dh_compat_info.is_none() {
        return Ok((false, None));
    }

    let (dh_compat_value, _line_no) = dh_compat_info.unwrap();

    // Compare to determine if there's a conflict
    let issue = match compat_version {
        Some(compat_ver) if dh_compat_value != compat_ver => {
            let issue = LintianIssue::source_with_info(
                "declares-possibly-conflicting-debhelper-compat-versions",
                vec![format!(
                    "{} vs elsewhere {} [{}]",
                    dh_compat_value, compat_ver, compat_source
                )],
            );

            if !issue.should_fix(base_path) {
                return Ok((false, Some(issue)));
            }
            Some(issue)
        }
        _ => None,
    };

    // Remove all DH_COMPAT definitions
    let dh_compat_defs: Vec<_> = makefile.find_variable("DH_COMPAT").collect();
    for mut def in dh_compat_defs {
        def.remove();
    }

    fs::write(rules_path, makefile.to_string())?;
    Ok((true, issue))
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let compat_path = base_path.join("debian/compat");
    let rules_path = base_path.join("debian/rules");

    // Read compat from debian/compat file
    let file_compat_version = read_debhelper_compat_file(&compat_path)?;

    // Read compat from debian/control
    let control_content = fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let control_compat_version = get_debhelper_compat_level_from_control(&control)?;

    // Determine which compat version to use and source
    let (compat_version, compat_source) = match (control_compat_version, file_compat_version) {
        (Some(control_ver), Some(_file_ver)) => {
            // Both exist - remove debian/compat and use control version
            fs::remove_file(&compat_path)?;
            (Some(control_ver), "debian/control")
        }
        (Some(control_ver), None) => (Some(control_ver), "debian/control"),
        (None, Some(file_ver)) => (Some(file_ver), "debian/compat"),
        (None, None) => return Err(FixerError::NoChanges),
    };

    // Update debian/rules to remove conflicting DH_COMPAT
    let (rules_changed, issue) = if rules_path.exists() {
        update_rules_file(&rules_path, compat_version, compat_source, base_path)?
    } else {
        (false, None)
    };

    // If nothing changed, return NoChanges
    if !(rules_changed || control_compat_version.is_some() && file_compat_version.is_some()) {
        if let Some(issue) = issue {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }
        return Err(FixerError::NoChanges);
    }

    // Build result with issues if there was a conflict
    let mut result = FixerResult::builder(
        "Avoid setting debhelper compat version in debian/rules and debian/compat.",
    );

    if let Some(issue) = issue {
        result = result.fixed_issues(vec![issue]);
    }

    Ok(result.build())
}

declare_fixer! {
    name: "declares-possibly-conflicting-debhelper-compat-versions",
    tags: ["declares-possibly-conflicting-debhelper-compat-versions"],
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
    fn test_read_compat_file() {
        let temp_dir = TempDir::new().unwrap();
        let compat_path = temp_dir.path().join("compat");

        fs::write(&compat_path, "11\n").unwrap();
        assert_eq!(read_debhelper_compat_file(&compat_path).unwrap(), Some(11));

        fs::write(&compat_path, "12").unwrap();
        assert_eq!(read_debhelper_compat_file(&compat_path).unwrap(), Some(12));
    }

    #[test]
    fn test_both_compat_sources_exist() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/control with debhelper-compat
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 10)\n\nPackage: blah\n",
        )
        .unwrap();

        // Create debian/compat
        fs::write(debian_dir.join("compat"), "11\n").unwrap();

        // Create debian/rules
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\n%:\n\tdh $@\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("Avoid setting debhelper compat version"));

        // debian/compat should be removed
        assert!(!debian_dir.join("compat").exists());
    }

    #[test]
    fn test_conflicting_dh_compat_in_rules() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create debian/control without debhelper-compat
        fs::write(
            debian_dir.join("control"),
            "Source: blah\nBuild-Depends: debhelper (>= 10.1)\n\nPackage: blah\n",
        )
        .unwrap();

        // Create debian/compat
        fs::write(debian_dir.join("compat"), "11\n").unwrap();

        // Create debian/rules with conflicting DH_COMPAT
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\n\nexport DH_COMPAT = 10\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result
            .description
            .contains("Avoid setting debhelper compat version"));

        // Check that DH_COMPAT line was removed from rules
        let rules_content = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(!rules_content.contains("export DH_COMPAT"));
        assert_eq!(rules_content, "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n");
    }
}
