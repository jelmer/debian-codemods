use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::{FixerError, LintianIssue};
use makefile_lossless::Makefile;
use std::path::{Path, PathBuf};

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let rules_abs = base_path.join(&rules_rel);
    if !rules_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&rules_abs)?;
    let parsed = Makefile::parse(&content);
    let makefile = parsed.tree();

    for var_def in makefile.variable_definitions() {
        let Some(name) = var_def.name() else {
            continue;
        };
        if name != "DEB_LDFLAGS_MAINT_APPEND" {
            continue;
        }
        let Some(raw_value) = var_def.raw_value() else {
            continue;
        };
        let Ok(args) = shell_words::split(raw_value.trim()) else {
            continue;
        };

        let mut new_args: Vec<String> = Vec::new();
        let mut found_as_needed = false;
        for arg in args {
            if let Some(rest) = arg.strip_prefix("-Wl,") {
                let kept: Vec<&str> = rest
                    .split(',')
                    .filter(|p| {
                        if *p == "--as-needed" {
                            found_as_needed = true;
                            false
                        } else {
                            true
                        }
                    })
                    .collect();
                if !kept.is_empty() {
                    new_args.push(format!("-Wl,{}", kept.join(",")));
                }
            } else {
                new_args.push(arg);
            }
        }

        if !found_as_needed {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "debian-rules-uses-as-needed-linker-flag",
            vec!["[debian/rules]".to_string()],
        );
        let action = if new_args.is_empty() {
            Action::Makefile(MakefileAction::RemoveVariable {
                file: rules_rel.clone(),
                name: name.clone(),
            })
        } else {
            Action::Makefile(MakefileAction::SetVariable {
                file: rules_rel.clone(),
                name: name.clone(),
                value: shell_words::join(&new_args),
            })
        };
        return Ok(vec![Diagnostic::with_actions(
            issue,
            "Avoid explicitly specifying -Wl,--as-needed linker flag.",
            vec![action],
        )]);
    }

    Ok(Vec::new())
}

declare_fixer! {
    name: "debian-rules-uses-as-needed-linker-flag",
    tags: ["debian-rules-uses-as-needed-linker-flag"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_as_needed_flag() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nexport DEB_LDFLAGS_MAINT_APPEND = -Wl,--as-needed\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_remove_as_needed_with_other_flags() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let rules = debian.join("rules");
        fs::write(
            &rules,
            "#!/usr/bin/make -f\n\nexport DEB_LDFLAGS_MAINT_APPEND = -Wl,--as-needed,-O1\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\nexport DEB_LDFLAGS_MAINT_APPEND = -Wl,-O1\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_no_changes_when_no_as_needed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\nexport DEB_LDFLAGS_MAINT_APPEND = -Wl,-O1\n\n%:\n\tdh $@\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
