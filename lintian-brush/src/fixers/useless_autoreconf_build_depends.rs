use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::rules::dh_invoke_drop_with;
use debian_control::lossless::relations::Relations;
use debversion::Version;
use makefile_lossless::Makefile;
use std::path::Path;

pub fn run(
    base_path: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");
    let control_path = base_path.join("debian/control");

    // Check if target release supports debhelper compat 10
    if let Some(compat_release) = preferences.compat_release.as_ref() {
        let max_version =
            debian_analyzer::debhelper::maximum_debhelper_compat_version(compat_release);
        if max_version < 10 {
            // Target release doesn't support debhelper 10
            return Err(FixerError::NoChanges);
        }
    }

    // Drop --with=autoreconf from debian/rules
    let content = std::fs::read_to_string(&rules_path)?;
    let makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;
    let mut made_changes = false;

    for mut rule in makefile.rules() {
        let mut commands_to_update = Vec::new();

        for (i, recipe) in rule.recipes().enumerate() {
            let new_recipe = dh_invoke_drop_with(&recipe, "autoreconf");

            if new_recipe != recipe {
                commands_to_update.push((i, new_recipe));
                made_changes = true;
            }
        }

        for (i, new_recipe) in commands_to_update {
            rule.replace_command(i, &new_recipe);
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    std::fs::write(&rules_path, makefile.to_string())?;

    // Ensure minimum debhelper version and drop dh-autoreconf dependency
    let editor = TemplatedControlEditor::open(&control_path)
        .map_err(|e| FixerError::Other(format!("Failed to open control: {}", e)))?;

    let Some(mut source) = editor.source() else {
        return Err(FixerError::NoChanges);
    };

    // Ensure minimum debhelper version 10~
    let minimum_version: Version = "10~".parse().unwrap();
    debian_analyzer::debhelper::ensure_minimum_debhelper_version(&mut source, &minimum_version)
        .map_err(|e| FixerError::Other(format!("{:?}", e)))?;

    // Drop dh-autoreconf build dependency
    let mut build_depends = source.build_depends().unwrap_or_else(Relations::new);
    let dropped = build_depends.drop_dependency("dh-autoreconf");

    if dropped {
        source.set_build_depends(&build_depends);
    }

    editor
        .commit()
        .map_err(|e| FixerError::Other(format!("Failed to commit: {}", e)))?;

    let issue = LintianIssue::source_with_info(
        "useless-autoreconf-build-depends",
        vec!["(does not need to satisfy dh-autoreconf:any)".to_string()],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    Ok(
        FixerResult::builder("Drop unnecessary dependency on dh-autoreconf.")
            .fixed_issue(issue)
            .build(),
    )
}

declare_fixer! {
    name: "useless-autoreconf-build-depends",
    tags: ["useless-autoreconf-build-depends"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_drop_autoreconf() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(
            &rules_path,
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=autoreconf\n",
        )
        .unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nBuild-Depends: debhelper (>= 9), dh-autoreconf\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path(), "blah", &FixerPreferences::default());
        assert!(result.is_ok());

        let rules_content = fs::read_to_string(&rules_path).unwrap();
        assert_eq!(rules_content, "#!/usr/bin/make -f\n\n%:\n\tdh $@\n");

        let control_content = fs::read_to_string(&control_path).unwrap();
        assert!(control_content.contains("debhelper (>= 10~"));
        assert!(!control_content.contains("dh-autoreconf"));
    }

    #[test]
    fn test_no_autoreconf() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, "#!/usr/bin/make -f\n\n%:\n\tdh $@\n").unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nBuild-Depends: debhelper (>= 10~)\n\nPackage: blah\n",
        )
        .unwrap();

        let result = run(temp_dir.path(), "blah", &FixerPreferences::default());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FixerError::NoChanges));
    }
}
