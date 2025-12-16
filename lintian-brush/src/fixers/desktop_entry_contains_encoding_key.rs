use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use desktop_edit::Desktop;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");

    if !debian_dir.exists() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues: Vec<(LintianIssue, String)> = Vec::new();
    let mut overridden_issues = Vec::new();

    let entries = match fs::read_dir(&debian_dir) {
        Ok(entries) => entries,
        Err(_) => return Err(FixerError::NoChanges),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(filename) = path.file_name() else {
            continue;
        };

        let Some(name) = filename.to_str() else {
            continue;
        };

        if !name.ends_with(".desktop") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        let desktop = Desktop::from_str(&content)
            .map_err(|e| FixerError::Other(format!("Failed to parse desktop file: {:?}", e)))?;

        let Some(mut group) = desktop.get_group("Desktop Entry") else {
            continue;
        };

        let Some(encoding) = group.get("Encoding") else {
            continue;
        };

        if encoding != "UTF-8" {
            continue;
        }

        // Find the Encoding entry to get its line number
        let encoding_entry = group
            .entries()
            .find(|e| e.key().as_deref() == Some("Encoding") && e.locale().is_none());

        let line_number = encoding_entry.map(|e| e.line()).unwrap_or(0);

        let relative_path = path
            .strip_prefix(base_path)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let issue = LintianIssue::source_with_info(
            "desktop-entry-contains-encoding-key",
            vec![
                "Encoding".to_string(),
                format!("[{}:{}]", relative_path, line_number),
            ],
        );

        if issue.should_fix(base_path) {
            group.remove("Encoding");
            fs::write(&path, desktop.to_string())?;
            fixed_issues.push((issue, relative_path));
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

    let fixed_paths: Vec<String> = fixed_issues.iter().map(|(_, path)| path.clone()).collect();
    let fixed_lintian_issues: Vec<LintianIssue> =
        fixed_issues.into_iter().map(|(issue, _)| issue).collect();

    let description = if fixed_paths.len() == 1 {
        format!(
            "Remove deprecated Encoding key from desktop file {}.",
            fixed_paths[0]
        )
    } else {
        format!(
            "Remove deprecated Encoding key from desktop files: {}.",
            fixed_paths.join(", ")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_lintian_issues)
        .overridden_issues(overridden_issues)
        .build())
}

declare_fixer! {
    name: "desktop-entry-contains-encoding-key",
    tags: ["desktop-entry-contains-encoding-key"],
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
    fn test_utf8() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("foo.desktop");
        fs::write(
            &desktop_path,
            "[Desktop Entry]\nType=Application\nEncoding=UTF-8\nName=XScreensaver\nTryExec=xscreensaver\nExec=/usr/share/xscreensaver/xscreensaver-wrapper.sh -nosplash\nNoDisplay=true\nX-KDE-StartupNotify=false\nComment=The XScreensaver daemon\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Remove deprecated Encoding key from desktop file debian/foo.desktop."
        );

        let content = fs::read_to_string(&desktop_path).unwrap();
        assert_eq!(
            content,
            "[Desktop Entry]\nType=Application\nName=XScreensaver\nTryExec=xscreensaver\nExec=/usr/share/xscreensaver/xscreensaver-wrapper.sh -nosplash\nNoDisplay=true\nX-KDE-StartupNotify=false\nComment=The XScreensaver daemon\n"
        );
    }

    #[test]
    fn test_no_desktop_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_encoding_key() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let desktop_path = debian_dir.join("foo.desktop");
        fs::write(
            &desktop_path,
            "[Desktop Entry]\nType=Application\nName=Test\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
