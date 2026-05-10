use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
enum ScriptStatus {
    Empty,
    SomeComments,
    NotEmpty,
}

const MAINTAINER_SCRIPTS: &[&str] = &["prerm", "postinst", "preinst", "postrm"];

fn classify(content: &[u8]) -> ScriptStatus {
    let mut status = ScriptStatus::Empty;

    for (line_no, line_bytes) in content.split(|&b| b == b'\n').enumerate() {
        let trimmed_line: &[u8] = {
            let mut end = line_bytes.len();
            while end > 0 && matches!(line_bytes[end - 1], b' ' | b'\t' | b'\r') {
                end -= 1;
            }
            &line_bytes[..end]
        };

        if trimmed_line.is_empty() {
            continue;
        }

        if line_no == 0 && trimmed_line.starts_with(b"#!") {
            continue;
        }

        if trimmed_line.starts_with(b"#") {
            let comment_content = &trimmed_line[1..];
            let comment_trimmed: &[u8] = {
                let mut start = 0;
                while start < comment_content.len() && comment_content[start] == b'#' {
                    start += 1;
                }
                &comment_content[start..]
            };

            if !comment_trimmed.is_empty() && trimmed_line != b"#DEBHELPER#" {
                status = ScriptStatus::SomeComments;
            }
            continue;
        }

        if trimmed_line.starts_with(b"set ") {
            continue;
        }

        if trimmed_line.starts_with(b"exit ") {
            continue;
        }

        return ScriptStatus::NotEmpty;
    }

    status
}

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

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    entries.sort();

    let mut diagnostics = Vec::new();

    for filename in entries {
        let Some((package, script)) = parse_maintainer_script_name(&filename) else {
            continue;
        };

        let rel = PathBuf::from("debian").join(&filename);
        let bytes = match ws.read_file(&rel)? {
            Some(b) => b,
            None => continue,
        };
        let status = classify(&bytes);
        match status {
            ScriptStatus::Empty | ScriptStatus::SomeComments => {
                let issue = if package == "source" {
                    LintianIssue::source_with_info(
                        "maintainer-script-empty",
                        Visibility::Warning,
                        vec![format!("[{}]", script)],
                    )
                } else {
                    LintianIssue::binary_with_info(
                        &package,
                        "maintainer-script-empty",
                        Visibility::Warning,
                        vec![format!("[{}]", script)],
                    )
                };

                let certainty = if status == ScriptStatus::SomeComments {
                    Certainty::Likely
                } else {
                    Certainty::Certain
                };

                let mut diag = Diagnostic::with_actions(
                    issue,
                    format!("Maintainer script {} ({}) is empty.", script, package),
                    format!("Remove empty maintainer script {} ({}).", script, package),
                    vec![Action::Filesystem(FilesystemAction::Delete { file: rel })],
                );
                diag = diag.with_certainty(certainty);
                diagnostics.push(diag);
            }
            ScriptStatus::NotEmpty => {}
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut entries: Vec<(String, String)> = fixed
        .iter()
        .filter_map(|(d, _)| {
            let issue = d.issue.as_ref()?;
            let info = issue
                .info
                .as_deref()?
                .trim_matches(|c| c == '[' || c == ']');
            let pkg = issue
                .package
                .clone()
                .unwrap_or_else(|| "source".to_string());
            Some((pkg, info.to_string()))
        })
        .collect();
    entries.sort();
    entries.dedup();
    let parts: Vec<String> = entries
        .into_iter()
        .map(|(pkg, script)| format!("{} ({})", pkg, script))
        .collect();
    format!("Remove empty maintainer scripts: {}", parts.join(", "))
}

declare_detector! {
    name: "maintainer-script-empty",
    tags: ["maintainer-script-empty"],
    triggers: [
        crate::workspace::Trigger::File("debian/preinst"),
        crate::workspace::Trigger::File("debian/postinst"),
        crate::workspace::Trigger::File("debian/prerm"),
        crate::workspace::Trigger::File("debian/postrm"),
        crate::workspace::Trigger::Glob("debian/*.preinst"),
        crate::workspace::Trigger::Glob("debian/*.postinst"),
        crate::workspace::Trigger::Glob("debian/*.prerm"),
        crate::workspace::Trigger::Glob("debian/*.postrm"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_classify_truly_empty() {
        assert_eq!(classify(b""), ScriptStatus::Empty);
    }

    #[test]
    fn test_classify_shebang_only() {
        assert_eq!(classify(b"#!/bin/sh\n"), ScriptStatus::Empty);
    }

    #[test]
    fn test_classify_comments_only() {
        assert_eq!(
            classify(b"#!/bin/sh\n# This is a comment\nset -e\n#DEBHELPER#\n"),
            ScriptStatus::SomeComments
        );
    }

    #[test]
    fn test_classify_has_content() {
        assert_eq!(
            classify(b"#!/bin/sh\necho 'Hello world'\n"),
            ScriptStatus::NotEmpty
        );
    }

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
    fn test_remove_empty_script() {
        let tmp = TempDir::new().unwrap();
        let debian_dir = tmp.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(debian_dir.join("mon.prerm"), "").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!debian_dir.join("mon.prerm").exists());
        assert_eq!(
            result.description,
            "Remove empty maintainer scripts: mon (prerm)"
        );
        assert_eq!(result.certainty, Some(Certainty::Certain));
    }

    #[test]
    fn test_remove_comments_only_script() {
        let tmp = TempDir::new().unwrap();
        let debian_dir = tmp.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("mon.prerm"),
            "#!/bin/sh\n# This is just a comment\nset -e\n#DEBHELPER#\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(!debian_dir.join("mon.prerm").exists());
        assert_eq!(result.certainty, Some(Certainty::Likely));
    }

    #[test]
    fn test_keep_non_empty_script() {
        let tmp = TempDir::new().unwrap();
        let debian_dir = tmp.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("mon.prerm"),
            "#!/bin/sh\necho 'This script does something'\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert!(debian_dir.join("mon.prerm").exists());
    }

    #[test]
    fn test_no_debian_directory() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
