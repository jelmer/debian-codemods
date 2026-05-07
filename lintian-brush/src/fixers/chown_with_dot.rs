use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use regex::Regex;
use std::path::{Path, PathBuf};

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

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let chown_regex = Regex::new(r"\bchown\s+([a-zA-Z0-9_-]+)\.([a-zA-Z0-9_-]+)\b").unwrap();
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
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
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

        let new_content = chown_regex.replace_all(&content, "chown $1:$2").to_string();

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                format!(
                    "Replace deprecated chown user.group with chown user:group in {} ({})",
                    package, script
                ),
                vec![Action::Filesystem(FilesystemAction::Write {
                    file: rel,
                    content: new_content.into_bytes(),
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

/// Aggregate when more than one script is touched: keep the original
/// "in N scripts" wording the historical fixer produced.
fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    if fixed.len() == 1 {
        fixed[0].message.clone()
    } else {
        format!(
            "Replace deprecated chown user.group with chown user:group in {} scripts",
            fixed.len()
        )
    }
}

declare_detector! {
    name: "chown-with-dot",
    tags: ["chown-with-dot"],
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
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
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
    fn test_fix_chown_with_dot() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("postinst"),
            "#!/bin/sh\nset -e\nchown root.root /etc/myconfig\nchown user-name.group-name /var/lib/myapp\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            result.description,
            "Replace deprecated chown user.group with chown user:group in source (postinst)"
        );

        assert_eq!(
            fs::read_to_string(debian_dir.join("postinst")).unwrap(),
            "#!/bin/sh\nset -e\nchown root:root /etc/myconfig\nchown user-name:group-name /var/lib/myapp\n",
        );
    }

    #[test]
    fn test_no_chown_with_dot() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(
            debian_dir.join("postinst"),
            "#!/bin/sh\nset -e\nchown root:root /etc/myconfig\n",
        )
        .unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
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

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(
            result.description,
            "Replace deprecated chown user.group with chown user:group in 2 scripts"
        );

        assert_eq!(
            fs::read_to_string(debian_dir.join("postinst")).unwrap(),
            "#!/bin/sh\nchown root:root /etc/config\n",
        );
        assert_eq!(
            fs::read_to_string(debian_dir.join("mypackage.preinst")).unwrap(),
            "#!/bin/sh\nchown www-data:www-data /var/www\n",
        );
    }

    #[test]
    fn test_preserve_other_dots() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("postinst"),
            "#!/bin/sh\n# Fix chown root.root but keep file.txt\nchown root.root /etc/file.txt\ncp config.old config.new\n",
        )
        .unwrap();

        run_apply(temp_dir.path()).unwrap();

        // The regex matches `chown user.group` anywhere — including in the
        // comment — so both occurrences become `chown user:group`.
        // Unrelated dotted tokens like `file.txt` and `config.old` are left
        // alone because they don't follow `chown`.
        assert_eq!(
            fs::read_to_string(debian_dir.join("postinst")).unwrap(),
            "#!/bin/sh\n# Fix chown root:root but keep file.txt\nchown root:root /etc/file.txt\ncp config.old config.new\n",
        );
    }

    #[test]
    fn test_no_debian_directory() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
