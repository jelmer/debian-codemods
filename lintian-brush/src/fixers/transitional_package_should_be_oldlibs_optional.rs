use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut packages = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    let default_priority = editor
        .source()
        .and_then(|s| s.as_deb822().get("Priority").map(|s| s.to_string()));

    let binaries: Vec<_> = editor.binaries().collect();

    for mut binary in binaries {
        let paragraph = binary.as_mut_deb822();

        // Skip udebs
        if let Some(package_type) = paragraph.get("Package-Type") {
            if package_type.trim() == "udeb" {
                continue;
            }
        }

        // Check if description contains "transitional package"
        let description = paragraph.get("Description").unwrap_or_default();
        if !description.to_lowercase().contains("transitional package") {
            continue;
        }

        let package_name = paragraph.get("Package").unwrap_or_default().to_string();

        // Get old section - from binary or source
        let old_section = if let Some(section) = paragraph.get("Section") {
            Some(section.to_string())
        } else {
            editor
                .source()
                .and_then(|s| s.as_deb822().get("Section").map(|s| s.to_string()))
        };

        // Get old priority - from binary or source
        let old_priority = if let Some(priority) = paragraph.get("Priority") {
            priority.to_string()
        } else {
            default_priority
                .as_deref()
                .unwrap_or("optional")
                .to_string()
        };

        // Create info string showing old section/priority
        let info = format!(
            "{}/{}",
            old_section.as_deref().unwrap_or("misc"),
            old_priority
        );

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "transitional-package-not-oldlibs-optional",
            vec![info],
        );

        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
            continue;
        }

        // Determine new section
        let new_section = if let Some(old_section) = old_section.as_ref() {
            if let Some((area, _section)) = old_section.split_once('/') {
                format!("{}/oldlibs", area)
            } else {
                "oldlibs".to_string()
            }
        } else {
            "oldlibs".to_string()
        };

        paragraph.set("Section", &new_section);

        // Handle priority
        if default_priority.as_deref() != Some("optional") {
            paragraph.set("Priority", "optional");
        } else {
            // If source priority is already optional, remove from binary
            paragraph.remove("Priority");
        }

        packages.push(package_name);
        fixed_issues.push(issue);
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let message = if packages.len() == 1 {
        format!(
            "Move transitional package {} to oldlibs/optional per policy 4.0.1.",
            packages[0]
        )
    } else {
        format!(
            "Move transitional packages {} to oldlibs/optional per policy 4.0.1.",
            packages.join(", ")
        )
    };

    Ok(FixerResult::builder(&message)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "transitional-package-should-be-oldlibs-optional",
    tags: ["transitional-package-not-oldlibs-optional"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_transitional_package_simple() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nPriority: standard\nSection: libs\nDescription: transitional package for blah\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Section: oldlibs"));
        assert!(!updated_content.contains("Priority: standard"));
    }

    #[test]
    fn test_transitional_package_with_area() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nPriority: standard\nSection: contrib/libs\nDescription: transitional package for blah\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains("Section: contrib/oldlibs"));
        assert!(!updated_content.contains("Priority: standard"));
    }

    #[test]
    fn test_skip_udeb() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: gdk-pixbuf\nSection: libs\nPriority: optional\n\nPackage: libgdk-pixbuf2.0-0-udeb\nPackage-Type: udeb\nSection: debian-installer\nDescription: GDK Pixbuf library - minimal runtime\n This transitional package depends on libgdk-pixbuf-2.0-0-udeb.\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(temp_dir.path(), "gdk-pixbuf", &version, &Default::default());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_transitional() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content =
            "Source: lintian-brush\nPriority: optional\n\nPackage: lintian-brush\nSection: libs\nDescription: A real package\n Test test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "lintian-brush",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_file() {
        let temp_dir = TempDir::new().unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
