use crate::declare_detector;
use crate::diagnostic::{
    Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector, TextRange,
};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use regex::bytes::Regex;
use std::path::{Path, PathBuf};

struct ConfigureMatch {
    range: TextRange,
    replacement: String,
    line_number: usize,
    used_pkg_prog: bool,
}

fn collect_matches(content: &[u8]) -> Vec<ConfigureMatch> {
    // Pattern: \s*AC_PATH_PROG\s*\(\s*(\[)?(?P<variable>[A-Z_]+)(\])?\s*,\s*(\[)?pkg-config(\])?\s*(,\s*(\[)?(?P<default>.*)(\])?\s*)?\)
    let re = Regex::new(
        r"(?m)^\s*AC_PATH_PROG\s*\(\s*(\[)?(?P<variable>[A-Z_]+)(\])?\s*,\s*(\[)?pkg-config(\])?\s*(,\s*(\[)?(?P<default>.*)(\])?\s*)?\)\n",
    )
    .unwrap();

    let mut matches = Vec::new();
    for caps in re.captures_iter(content) {
        let m = caps.get(0).unwrap();
        let variable = caps.name("variable").unwrap().as_bytes();
        let default = caps.name("default").map(|d| d.as_bytes());
        let line_number = content[..m.start()].iter().filter(|&&b| b == b'\n').count() + 1;

        let (replacement, used_pkg_prog) = if variable == b"PKG_CONFIG" && default.is_none() {
            ("PKG_PROG_PKG_CONFIG\n".to_string(), true)
        } else {
            // Replace only the macro name within the matched bytes.
            let original = std::str::from_utf8(m.as_bytes())
                .unwrap_or_default()
                .to_string();
            (original.replacen("AC_PATH_PROG", "AC_PATH_TOOL", 1), false)
        };

        matches.push(ConfigureMatch {
            range: TextRange {
                start: m.start(),
                end: m.end(),
            },
            replacement,
            line_number,
            used_pkg_prog,
        });
    }
    matches
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // Try both configure.ac and configure.in.
    let candidates = ["configure.ac", "configure.in"];
    let mut hit_file: Option<&str> = None;
    let mut matches: Vec<ConfigureMatch> = Vec::new();
    for name in &candidates {
        let content = match ws.read_file(Path::new(name))? {
            Some(c) => c,
            None => continue,
        };
        let m = collect_matches(&content);
        if !m.is_empty() {
            hit_file = Some(name);
            matches = m;
            break;
        }
    }
    let Some(file_name) = hit_file else {
        return Ok(Vec::new());
    };
    let file_rel = PathBuf::from(file_name);

    let any_pkg_prog = matches.iter().any(|m| m.used_pkg_prog);
    let any_path_tool = matches.iter().any(|m| !m.used_pkg_prog);
    let resolution = if any_pkg_prog && !any_path_tool {
        "This patch changes it to use PKG_PROG_PKG_CONFIG macro from pkg.m4."
    } else if any_path_tool && !any_pkg_prog {
        "This patch changes it to use AC_PATH_TOOL."
    } else {
        // Mixed — report the second one (matches the legacy single-string
        // resolution that gets overwritten by later iterations).
        "This patch changes it to use AC_PATH_TOOL."
    };
    let description = if any_pkg_prog && !any_path_tool {
        "AC_PATH_PROG is not cross-compilation safe; use PKG_PROG_PKG_CONFIG."
    } else {
        "AC_PATH_PROG is not cross-compilation safe; use AC_PATH_TOOL."
    };
    let first_line = matches[0].line_number;
    let issue = LintianIssue::source_with_info(
        "autotools-pkg-config-macro-not-cross-compilation-safe",
        Visibility::Warning,
        vec![format!("AC_PATH_PROG [{}:{}]", file_name, first_line)],
    );
    let label = format!(
        "Use cross-build compatible macro for finding pkg-config.\n\n\
         The package uses AC_PATH_PROG to discover the location of pkg-config(1). This\n\
         macro fails to select the correct version to support cross-compilation.\n\n\
         {}\n\n\
         Refer to https://bugs.debian.org/884798 for details.\n",
        resolution
    );

    // Apply edits in reverse-offset order so earlier-offset replacements
    // don't shift later ones.
    let mut sorted = matches;
    sorted.sort_by(|a, b| b.range.start.cmp(&a.range.start));
    let mut actions: Vec<Action> = sorted
        .into_iter()
        .map(|m| {
            Action::Filesystem(FilesystemAction::ReplaceText {
                file: file_rel.clone(),
                range: m.range,
                replacement: m.replacement,
            })
        })
        .collect();

    // If we used PKG_PROG_PKG_CONFIG, the package now needs pkg-config in
    // Build-Depends.
    if any_pkg_prog {
        let control_rel = PathBuf::from("debian/control");
        if let Ok(control) = ws.parsed_control() {
            if let Some(source) = control.source() {
                let bd = source.as_deb822().get("Build-Depends").unwrap_or_default();
                if !bd
                    .split(',')
                    .any(|e| e.trim().split_whitespace().next() == Some("pkg-config"))
                {
                    actions.push(Action::Deb822(Deb822Action::EnsureRelation {
                        file: control_rel,
                        paragraph: ParagraphSelector::Source,
                        field: "Build-Depends".into(),
                        entry: "pkg-config".into(),
                    }));
                }
            }
        }
    }

    Ok(vec![Diagnostic::with_actions(
        issue,
        description,
        label,
        actions,
    )
    .with_patch_name("ac-path-pkgconfig")])
}

declare_detector! {
    name: "autotools-pkg-config-macro-not-cross-compilation-safe",
    tags: ["autotools-pkg-config-macro-not-cross-compilation-safe"],
    triggers: [
        debian_workspace::Trigger::File("configure.ac"),
        debian_workspace::Trigger::File("configure.in"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    #[test]
    fn test_replace_ac_path_prog_with_pkg_prog_pkg_config() {
        let tmp = TempDir::new().unwrap();
        let configure_ac = tmp.path().join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\nAC_PATH_PROG([PKG_CONFIG], [pkg-config])\nAC_OUTPUT\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(
            result
                .description
                .contains("PKG_PROG_PKG_CONFIG macro from pkg.m4"),
            "description: {}",
            result.description
        );
        assert_eq!(
            fs::read_to_string(&configure_ac).unwrap(),
            "AC_INIT([test], [1.0])\nPKG_PROG_PKG_CONFIG\nAC_OUTPUT\n",
        );
    }

    #[test]
    fn test_replace_ac_path_prog_with_ac_path_tool() {
        let tmp = TempDir::new().unwrap();
        let configure_ac = tmp.path().join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\nAC_PATH_PROG([PKGCONFIG], [pkg-config])\nAC_OUTPUT\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert!(result.description.contains("AC_PATH_TOOL"));
        assert_eq!(
            fs::read_to_string(&configure_ac).unwrap(),
            "AC_INIT([test], [1.0])\nAC_PATH_TOOL([PKGCONFIG], [pkg-config])\nAC_OUTPUT\n",
        );
    }

    #[test]
    fn test_replace_ac_path_prog_with_default() {
        let tmp = TempDir::new().unwrap();
        let configure_ac = tmp.path().join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\nAC_PATH_PROG([PKG_CONFIG], [pkg-config], [no])\nAC_OUTPUT\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&configure_ac).unwrap(),
            "AC_INIT([test], [1.0])\nAC_PATH_TOOL([PKG_CONFIG], [pkg-config], [no])\nAC_OUTPUT\n",
        );
    }

    #[test]
    fn test_no_changes_when_no_ac_path_prog() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("configure.ac"),
            b"AC_INIT([test], [1.0])\nPKG_PROG_PKG_CONFIG\nAC_OUTPUT\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_configure_ac() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_updates_build_depends_for_pkg_prog_pkg_config() {
        let tmp = TempDir::new().unwrap();
        let configure_ac = tmp.path().join("configure.ac");
        fs::write(
            &configure_ac,
            b"AC_INIT([test], [1.0])\nAC_PATH_PROG([PKG_CONFIG], [pkg-config])\nAC_OUTPUT\n",
        )
        .unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            b"Source: test\nMaintainer: Test <test@example.com>\nBuild-Depends: debhelper\n\nPackage: test\nDescription: Test package\n Test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert!(fs::read_to_string(&control).unwrap().contains("pkg-config"));
    }
}
