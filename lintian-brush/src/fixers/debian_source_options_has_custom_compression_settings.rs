use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType};
use std::collections::BTreeSet;
use std::path::PathBuf;

const SEP: char = '\t';

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let options_rel = PathBuf::from("debian/source/options");
    let bytes = match ws.read_file(&options_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(Vec::new());
    };
    let oldlines: Vec<&str> = content.lines().collect();

    let mut newlines: Vec<String> = Vec::new();
    let mut dropped: BTreeSet<String> = BTreeSet::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for (lineno, line) in oldlines.iter().enumerate() {
        if line.trim_start().starts_with('#') {
            newlines.push(line.to_string());
            continue;
        }
        let key = line.find('=').map(|p| line[..p].trim()).unwrap_or("");
        match key {
            "compression" | "compression-level" => {
                let issue = LintianIssue {
                    package: None,
                    package_type: Some(PackageType::Source),
                    tag: Some("custom-compression-in-debian-source-options".to_string()),
                    info: Some(format!("{} (line {})", line, lineno + 1)),
                };
                // Drop any preceding contiguous comment lines.
                while !newlines.is_empty() && newlines.last().unwrap().trim_start().starts_with('#')
                {
                    newlines.pop();
                }
                let label = if key == "compression-level" {
                    "custom source compression level"
                } else {
                    "custom source compression"
                };
                dropped.insert(label.to_string());
                // The action set is filled in below (after we know the
                // final file content).
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!("drop{}{}", SEP, label),
                    Vec::new(),
                ));
            }
            _ => newlines.push(line.to_string()),
        }
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    // Decide whether the file should be deleted (became empty) or
    // rewritten with the surviving lines. Attach the resulting action
    // set to the first diagnostic.
    let action = if newlines.is_empty() {
        Action::Filesystem(FilesystemAction::Delete { file: options_rel })
    } else {
        let mut new_content = newlines.join("\n");
        new_content.push('\n');
        Action::Filesystem(FilesystemAction::Write {
            file: options_rel,
            content: new_content.into_bytes(),
        })
    };
    diagnostics[0].plans[0].actions.push(action);

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut labels: Vec<String> = fixed
        .iter()
        .filter_map(|d| {
            d.message
                .split_once(SEP)
                .filter(|(tag, _)| *tag == "drop")
                .map(|(_, lab)| lab.to_string())
        })
        .collect();
    labels.sort();
    labels.dedup();
    format!("Drop {}.", labels.join(", "))
}

declare_detector! {
    name: "debian-source-options-has-custom-compression-settings",
    tags: ["custom-compression-in-debian-source-options"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_removes_compression() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("debian/source");
        fs::create_dir_all(&source).unwrap();
        let options = source.join("options");
        fs::write(&options, "compression = xz\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!options.exists());
    }

    #[test]
    fn test_removes_compression_level() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("debian/source");
        fs::create_dir_all(&source).unwrap();
        let options = source.join("options");
        fs::write(&options, "compression-level = 9\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!options.exists());
    }

    #[test]
    fn test_keeps_other_options() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("debian/source");
        fs::create_dir_all(&source).unwrap();
        let options = source.join("options");
        fs::write(&options, "compression = xz\nother-option = value\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&options).unwrap(),
            "other-option = value\n"
        );
    }

    #[test]
    fn test_removes_prior_comments() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("debian/source");
        fs::create_dir_all(&source).unwrap();
        let options = source.join("options");
        fs::write(&options, "# Comment about compression\ncompression = xz\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(!options.exists());
    }

    #[test]
    fn test_no_change_when_no_custom_compression() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("debian/source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("options"), "other-option = value\n").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
