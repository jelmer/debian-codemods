use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::MultiArch;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut updated_packages = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut binary in editor.binaries() {
        // Get package name
        let package = match binary.name() {
            Some(p) => p.to_string(),
            None => {
                log::debug!("Skipping binary package without name");
                continue;
            }
        };

        // Check if it's a font package
        if !package.starts_with("fonts-") && !package.starts_with("xfonts-") {
            continue;
        }

        // Check architecture
        let arch = binary.architecture().map(|a| a.to_string());
        if !matches!(arch.as_deref(), Some("all") | None) {
            continue;
        }

        // Skip if Multi-Arch is already set
        if binary.multi_arch().is_some() {
            continue;
        }

        let issue =
            LintianIssue::binary_with_info(&package, "font-package-not-multi-arch-foreign", vec![]);

        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
            continue;
        }

        // Add Multi-Arch: foreign
        binary.set_multi_arch(Some(MultiArch::Foreign));
        updated_packages.push(package);
        fixed_issues.push(issue);
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    // Create the result message
    let plural = if updated_packages.len() > 1 { "s" } else { "" };
    let packages_str = updated_packages.join(", ");
    let message = format!(
        "Set Multi-Arch: foreign on package{} {}",
        plural, packages_str
    );

    Ok(FixerResult::builder(message)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "font-package-not-multi-arch-foreign",
    tags: ["font-package-not-multi-arch-foreign"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_add_multi_arch_foreign_to_font_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-blah\n\
            Package: fonts-blah\n\
            Architecture: all\n\
            Description: Test font package\n\
            \n\
            Package: ttf-blah\n\
            Architecture: all\n\
            Description: Transition package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on package fonts-blah"
        );

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Package: fonts-blah"));
        assert!(updated_content.contains("Multi-Arch: foreign"));
        // ttf-blah should not have Multi-Arch added
        let ttf_section = updated_content.split("Package: ttf-blah").nth(1).unwrap();
        assert!(!ttf_section.contains("Multi-Arch:"));
    }

    #[test]
    fn test_xfonts_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: xfonts-test\n\
            \n\
            Package: xfonts-test\n\
            Architecture: all\n\
            Description: X font package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on package xfonts-test"
        );
    }

    #[test]
    fn test_non_font_package() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: regular-package\n\
            \n\
            Package: regular-package\n\
            Architecture: all\n\
            Description: Regular package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_multi_arch() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-blah\n\
            \n\
            Package: fonts-blah\n\
            Architecture: all\n\
            Multi-Arch: foreign\n\
            Description: Test font package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_all_architecture() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-blah\n\
            \n\
            Package: fonts-blah\n\
            Architecture: amd64\n\
            Description: Test font package\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_font_packages() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: fonts-collection\n\
            \n\
            Package: fonts-foo\n\
            Architecture: all\n\
            Description: Foo font\n\
            \n\
            Package: fonts-bar\n\
            Architecture: all\n\
            Description: Bar font\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Set Multi-Arch: foreign on packages fonts-foo, fonts-bar"
        );

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Package: fonts-foo"));
        assert!(updated_content.contains("Package: fonts-bar"));
        // Check both have Multi-Arch: foreign
        let foo_section = updated_content
            .split("Package: fonts-foo")
            .nth(1)
            .unwrap()
            .split("Package:")
            .next()
            .unwrap();
        assert!(foo_section.contains("Multi-Arch: foreign"));
        let bar_section = updated_content.split("Package: fonts-bar").nth(1).unwrap();
        assert!(bar_section.contains("Multi-Arch: foreign"));
    }
}
