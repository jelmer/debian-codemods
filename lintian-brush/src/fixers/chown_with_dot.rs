use crate::{Certainty, FixerError, FixerResult, LintianIssue};
use regex::Regex;
use std::fs;
use std::path::Path;

const MAINTAINER_SCRIPTS: &[&str] = &["prerm", "postinst", "preinst", "postrm"];

fn parse_maintainer_script_name(filename: &str) -> Option<(String, String)> {
    if MAINTAINER_SCRIPTS.contains(&filename) {
        return Some(("source".to_string(), filename.to_string()));
    }

    if let Some(dot_pos) = filename.rfind('.') {
        let package = &filename[..dot_pos];
        let script = &filename[dot_pos + 1..];

        if MAINTAINER_SCRIPTS.contains(&script) {
            return Some((package.to_string(), script.to_string()));
        }
    }

    None
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let chown_regex = Regex::new(r"\bchown\s+([a-zA-Z0-9_-]+)\.([a-zA-Z0-9_-]+)\b").unwrap();

    let mut fixed_scripts = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    let entries = fs::read_dir(&debian_dir)?;

    for entry in entries {
        let entry = entry?;
        let filename = entry.file_name().to_string_lossy().to_string();

        if let Some((package, script)) = parse_maintainer_script_name(&filename) {
            let script_path = entry.path();

            if !script_path.is_file() {
                continue;
            }

            let content = fs::read_to_string(&script_path)?;

            if !chown_regex.is_match(&content) {
                continue;
            }

            let issue = if package == "source" {
                LintianIssue::source_with_info("chown-with-dot", vec![format!("[{}]", script)])
            } else {
                LintianIssue::binary_with_info(
                    &package,
                    "chown-with-dot",
                    vec![format!("[{}]", script)],
                )
            };

            if !issue.should_fix(base_path) {
                overridden_issues.push(issue);
                continue;
            }

            let new_content = chown_regex.replace_all(&content, "chown $1:$2");
            fs::write(&script_path, new_content.as_ref())?;

            fixed_scripts.push((package, script));
            fixed_issues.push(issue);
        }
    }

    if fixed_scripts.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    let description = if fixed_scripts.len() == 1 {
        let (package, script) = &fixed_scripts[0];
        format!(
            "Replace deprecated chown user.group with chown user:group in {} ({})",
            package, script
        )
    } else {
        format!(
            "Replace deprecated chown user.group with chown user:group in {} scripts",
            fixed_scripts.len()
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .overridden_issues(overridden_issues)
        .certainty(Certainty::Certain)
        .build())
}

declare_fixer! {
    name: "chown-with-dot",
    tags: ["chown-with-dot"],
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
    fn test_parse_maintainer_script_name() {
        assert_eq!(
            parse_maintainer_script_name("prerm"),
            Some(("source".to_string(), "prerm".to_string()))
        );

        assert_eq!(
            parse_maintainer_script_name("mypackage.postinst"),
            Some(("mypackage".to_string(), "postinst".to_string()))
        );

        assert_eq!(parse_maintainer_script_name("not_a_script"), None);
        assert_eq!(parse_maintainer_script_name("package.unknown"), None);
    }

    #[test]
    fn test_fix_chown_with_dot() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let script_content = r#"#!/bin/sh
set -e
chown root.root /etc/myconfig
chown user-name.group-name /var/lib/myapp
"#;
        fs::write(debian_dir.join("postinst"), script_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let new_content = fs::read_to_string(debian_dir.join("postinst")).unwrap();
        assert!(new_content.contains("chown root:root"));
        assert!(new_content.contains("chown user-name:group-name"));
        assert!(!new_content.contains("chown root.root"));
        assert!(!new_content.contains("chown user-name.group-name"));

        let result = result.unwrap();
        assert!(result
            .description
            .contains("Replace deprecated chown user.group with chown user:group"));
        assert_eq!(result.certainty, Some(Certainty::Certain));
    }

    #[test]
    fn test_no_chown_with_dot() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let script_content = r#"#!/bin/sh
set -e
chown root:root /etc/myconfig
"#;
        fs::write(debian_dir.join("postinst"), script_content).unwrap();

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

    #[test]
    fn test_multiple_scripts() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("postinst"),
            "#!/bin/sh\nchown root.root /etc/config\n",
        )
        .unwrap();
        fs::write(
            debian_dir.join("mypackage.preinst"),
            "#!/bin/sh\nchown www-data.www-data /var/www\n",
        )
        .unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let result = result.unwrap();
        assert!(result.description.contains("2 scripts"));
    }

    #[test]
    fn test_preserve_other_dots() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let script_content = r#"#!/bin/sh
# Fix chown root.root but keep file.txt
chown root.root /etc/file.txt
cp config.old config.new
"#;
        fs::write(debian_dir.join("postinst"), script_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let new_content = fs::read_to_string(debian_dir.join("postinst")).unwrap();
        assert!(new_content.contains("chown root:root /etc/file.txt"));
        assert!(new_content.contains("file.txt"));
        assert!(new_content.contains("config.old config.new"));
    }

    #[test]
    fn test_no_debian_directory() {
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
