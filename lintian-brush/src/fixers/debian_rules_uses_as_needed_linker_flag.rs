use crate::{declare_fixer, FixerError, FixerResult};
use makefile_lossless::Makefile;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let parsed = Makefile::parse(&content);
    let makefile = parsed.tree();

    let mut made_changes = false;

    // Process all variable definitions
    for mut var_def in makefile.variable_definitions() {
        let Some(name) = var_def.name() else {
            continue;
        };

        if name != "DEB_LDFLAGS_MAINT_APPEND" {
            continue;
        }

        let Some(raw_value) = var_def.raw_value() else {
            continue;
        };

        let trimmed_value = raw_value.trim();
        let Ok(args) = shell_words::split(trimmed_value) else {
            continue;
        };

        let mut new_args: Vec<String> = Vec::new();
        let mut found_as_needed = false;

        for arg in args {
            if arg.starts_with("-Wl") {
                let ld_parts: Vec<&str> = arg.split(',').collect();
                let new_ld_parts: Vec<&str> = ld_parts
                    .into_iter()
                    .filter(|&part| {
                        if part == "--as-needed" {
                            found_as_needed = true;
                            false
                        } else {
                            true
                        }
                    })
                    .collect();

                // Only add if we have more than just "-Wl"
                if new_ld_parts.len() > 1 {
                    new_args.push(new_ld_parts.join(","));
                }
            } else {
                new_args.push(arg);
            }
        }

        if !found_as_needed {
            continue;
        }

        made_changes = true;

        if new_args.is_empty() {
            // Remove the entire variable definition
            var_def.remove();
        } else {
            // Update the value
            let new_value = shell_words::join(&new_args);
            var_def.set_value(&new_value);
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    fs::write(&rules_path, makefile.code())?;

    Ok(
        FixerResult::builder("Avoid explicitly specifying -Wl,--as-needed linker flag.")
            .fixed_tag("debian-rules-uses-as-needed-linker-flag")
            .build(),
    )
}

declare_fixer! {
    name: "debian-rules-uses-as-needed-linker-flag",
    tags: ["debian-rules-uses-as-needed-linker-flag"],
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
    fn test_remove_as_needed_flag() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

export DEB_LDFLAGS_MAINT_APPEND = -Wl,--as-needed

%:
	dh $@
"#;
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Error: {:?}", result);

        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("--as-needed"));
        assert!(!updated_content.contains("DEB_LDFLAGS_MAINT_APPEND"));
    }

    #[test]
    fn test_remove_as_needed_with_other_flags() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

export DEB_LDFLAGS_MAINT_APPEND = -Wl,--as-needed,-O1

%:
	dh $@
"#;
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let result = run(temp_dir.path());
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("--as-needed"));
        assert!(updated_content.contains("DEB_LDFLAGS_MAINT_APPEND"));
        assert!(updated_content.contains("-Wl,-O1"));
    }

    #[test]
    fn test_no_changes_when_no_as_needed() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = r#"#!/usr/bin/make -f

export DEB_LDFLAGS_MAINT_APPEND = -Wl,-O1

%:
	dh $@
"#;
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
