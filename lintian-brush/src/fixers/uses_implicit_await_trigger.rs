use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, TriggersAction};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

/// Trigger types whose semantics are "await" even though the name doesn't say
/// so. Renaming to the explicit `-await` form preserves behaviour and
/// silences `uses-implicit-await-trigger`.
const IMPLICIT_AWAIT_TYPES: &[&str] = &["activate", "interest"];

/// A single trigger line, with the type extracted for inspection.
struct TriggerLine {
    /// 1-based line number in the source file.
    line_no: usize,
    /// Trigger type (first whitespace-delimited token).
    trigger_type: String,
}

fn parse_triggers(content: &str) -> Vec<TriggerLine> {
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trigger_type = trimmed.split_whitespace().next().unwrap_or("").to_string();
        if trigger_type.is_empty() {
            continue;
        }
        out.push(TriggerLine {
            line_no: idx + 1,
            trigger_type,
        });
    }
    out
}

/// Resolve the binary package name a `debian/<name>` triggers file applies
/// to. Returns `None` if the single-package `debian/triggers` form is used
/// and no suitable binary could be derived from `debian/control`.
fn binary_for_triggers_file(ws: &dyn Workspace, file_name: &str) -> Option<String> {
    if let Some(pkg) = file_name.strip_suffix(".triggers") {
        return Some(pkg.to_string());
    }
    // Bare "debian/triggers": only valid when there is exactly one binary.
    let control = ws.parsed_control().ok()?;
    let mut binaries = control.binaries();
    let first = binaries.next()?;
    if binaries.next().is_some() {
        return None;
    }
    first.as_deb822().get("Package")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let mut diagnostics = Vec::new();
    for name in entries {
        if name != "triggers" && !name.ends_with(".triggers") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&name);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };

        let Some(package) = binary_for_triggers_file(ws, &name) else {
            continue;
        };

        for trigger in parse_triggers(content) {
            if !IMPLICIT_AWAIT_TYPES.contains(&trigger.trigger_type.as_str()) {
                continue;
            }
            let new_type = format!("{}-await", trigger.trigger_type);
            let info_pointer = format!("triggers:{}", trigger.line_no);
            let issue = LintianIssue::binary_with_info(
                package.clone(),
                "uses-implicit-await-trigger",
                Visibility::Warning,
                vec![info_pointer, trigger.trigger_type.clone()],
            );
            diagnostics.push(Diagnostic::with_actions(
                issue,
                format!(
                    "{} uses implicit-await trigger type {}.",
                    rel.display(),
                    trigger.trigger_type
                ),
                format!(
                    "Make {} trigger explicit by using {} instead.",
                    trigger.trigger_type, new_type
                ),
                vec![Action::Triggers(TriggersAction::ReplaceType {
                    file: rel.clone(),
                    line: trigger.line_no,
                    from: trigger.trigger_type.clone(),
                    to: new_type,
                })],
            ));
        }
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "uses-implicit-await-trigger",
    tags: ["uses-implicit-await-trigger"],
    triggers: [
        debian_workspace::Trigger::File("debian/triggers"),
        debian_workspace::Trigger::Glob("debian/*.triggers"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use tempfile::tempdir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = FsWorkspace::new(base, Some("test".into()), Some(version.clone()));
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_simple_control(debian: &Path, binary: &str) {
        fs::write(
            debian.join("control"),
            format!(
                "Source: {}\nSection: misc\nPriority: optional\n\nPackage: {}\nArchitecture: all\nDescription: test\n",
                binary, binary
            ),
        )
        .unwrap();
    }

    #[test]
    fn parse_skips_comments_and_blank_lines() {
        let content = "# a comment\n\n  interest /usr/share/foo\nactivate /usr/share/bar\n";
        let lines = parse_triggers(content);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_no, 3);
        assert_eq!(lines[0].trigger_type, "interest");
        assert_eq!(lines[1].line_no, 4);
        assert_eq!(lines[1].trigger_type, "activate");
    }

    #[test]
    fn parse_ignores_leading_whitespace() {
        let content = "   activate /path\n\t\tinterest /other\n";
        let lines = parse_triggers(content);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].trigger_type, "activate");
        assert_eq!(lines[1].trigger_type, "interest");
    }

    #[test]
    fn detect_emits_action_for_implicit_activate() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");
        fs::write(debian.join("triggers"), "activate /usr/share/foo\n").unwrap();

        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Triggers(TriggersAction::ReplaceType {
                file: PathBuf::from("debian/triggers"),
                line: 1,
                from: "activate".into(),
                to: "activate-await".into(),
            })
        );
        assert_eq!(
            diags[0].issue.as_ref().unwrap().info.as_deref(),
            Some("triggers:1 activate")
        );
        assert_eq!(
            diags[0].issue.as_ref().unwrap().package.as_deref(),
            Some("foo"),
        );
    }

    #[test]
    fn detect_handles_per_binary_triggers_file() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");
        fs::write(debian.join("foo.triggers"), "interest /etc/foo\n").unwrap();

        let diags = detect_in(tmp.path()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].issue.as_ref().unwrap().package.as_deref(),
            Some("foo"),
        );
        assert_eq!(
            diags[0].plans[0].actions[0],
            Action::Triggers(TriggersAction::ReplaceType {
                file: PathBuf::from("debian/foo.triggers"),
                line: 1,
                from: "interest".into(),
                to: "interest-await".into(),
            }),
        );
    }

    #[test]
    fn detect_skips_explicit_await_and_noawait() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");
        fs::write(
            debian.join("triggers"),
            "activate-await /a\nactivate-noawait /b\ninterest-await /c\ninterest-noawait /d\n",
        )
        .unwrap();

        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn apply_rewrites_only_the_matched_line() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");
        let path = debian.join("triggers");
        fs::write(
            &path,
            "# header comment\n\nactivate /usr/share/foo\ninterest-await /etc/bar\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# header comment\n\nactivate-await /usr/share/foo\ninterest-await /etc/bar\n",
        );
    }

    #[test]
    fn no_changes_without_triggers_file() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");

        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn no_changes_when_no_debian_dir() {
        let tmp = tempdir().unwrap();
        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn skips_bare_triggers_in_multi_binary_package() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: multi\nSection: misc\nPriority: optional\n\nPackage: foo\nArchitecture: all\nDescription: foo\n\nPackage: bar\nArchitecture: all\nDescription: bar\n",
        )
        .unwrap();
        fs::write(debian.join("triggers"), "activate /usr/share/foo\n").unwrap();

        // Ambiguous ownership: don't guess, back off.
        assert!(detect_in(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn preserves_lines_with_trailing_content() {
        let tmp = tempdir().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        write_simple_control(&debian, "foo");
        let path = debian.join("triggers");
        fs::write(&path, "  activate   /path/with/spaces\n").unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "  activate-await   /path/with/spaces\n",
        );
    }
}
