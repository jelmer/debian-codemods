use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Extract the first `module <path>` declaration from a `go.mod` file.
///
/// Mirrors the regex lintian's `languages/golang/module-path` check uses
/// (`^ \s* module \s+ "? (\S+) "?`), stopping at the first match.
fn parse_module_path(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("module") else {
            continue;
        };
        if !rest.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let mut rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix('"') {
            rest = stripped;
        }
        let path: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"')
            .collect();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let source_para = source.as_deb822();

    // Lintian limits the check to packages that build-depend on
    // `golang-go | golang-any`.
    let is_go_package = source.build_depends().is_some_and(|bd| {
        bd.entries().any(|or_deps| {
            or_deps.relations().any(|dep| {
                matches!(
                    dep.try_name().as_deref(),
                    Some("golang-go") | Some("golang-any")
                )
            })
        })
    });
    if !is_go_package {
        return Ok(Vec::new());
    }

    if source_para.get("XS-Go-Import-Path").is_some() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "missing-xs-go-import-path-for-golang-package",
        Visibility::Info,
        Vec::new(),
    );
    let description = "Missing XS-Go-Import-Path field for Go package.".to_string();
    let label = "Set XS-Go-Import-Path in debian/control.".to_string();

    let module_path = ws
        .read_file(std::path::Path::new("go.mod"))
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes.into_owned()).ok())
        .as_deref()
        .and_then(parse_module_path);

    let Some(module_path) = module_path else {
        return Ok(vec![Diagnostic::with_actions(
            issue,
            description,
            label,
            vec![],
        )]);
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        description,
        label,
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "XS-Go-Import-Path".into(),
            value: module_path,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

fn describe(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let Some((diag, _)) = fixed.first() else {
        return "Set XS-Go-Import-Path in debian/control.".to_string();
    };
    let Some(plan) = diag.plans.first() else {
        return "Set XS-Go-Import-Path in debian/control.".to_string();
    };
    plan.label.clone()
}

declare_detector! {
    name: "missing-xs-go-import-path-for-golang-package",
    tags: ["missing-xs-go-import-path-for-golang-package"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "XS-Go-Import-Path",
        },
        debian_workspace::Trigger::File("go.mod"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("blah".into()),
            Some(v.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn parse_module_path_simple() {
        assert_eq!(
            parse_module_path("module github.com/foo/bar\n"),
            Some("github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn parse_module_path_quoted() {
        assert_eq!(
            parse_module_path("module \"github.com/foo/bar\"\n"),
            Some("github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn parse_module_path_with_leading_whitespace() {
        assert_eq!(
            parse_module_path("   module github.com/foo/bar\n"),
            Some("github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn parse_module_path_picks_first() {
        let content = "// comment\nmodule github.com/foo/bar\nmodule github.com/baz/qux\n";
        assert_eq!(
            parse_module_path(content),
            Some("github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn parse_module_path_skips_other_lines() {
        let content =
            "// top comment\n\nrequire github.com/foo/bar v1.2.3\nmodule example.com/mod\n";
        assert_eq!(
            parse_module_path(content),
            Some("example.com/mod".to_string())
        );
    }

    #[test]
    fn parse_module_path_none_when_absent() {
        assert_eq!(parse_module_path("go 1.21\n"), None);
    }

    #[test]
    fn parse_module_path_requires_module_keyword_followed_by_space() {
        assert_eq!(parse_module_path("modular foo\n"), None);
    }

    #[test]
    fn test_adds_field_from_go_mod() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-any\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(tmp.path().join("go.mod"), "module github.com/foo/bar\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: blah\nBuild-Depends: golang-any\nXS-Go-Import-Path: github.com/foo/bar\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }

    #[test]
    fn test_no_change_when_field_already_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-any\nXS-Go-Import-Path: example.com/existing\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(tmp.path().join("go.mod"), "module github.com/foo/bar\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_golang_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_without_go_mod() {
        // Without go.mod there is no reliable import path to fall back on;
        // the detector should still report the issue but offer no fix,
        // so apply() produces no file changes.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_go_mod_has_no_module_directive() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(tmp.path().join("go.mod"), "go 1.21\n").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_detects_golang_go() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: golang-go\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        )
        .unwrap();
        fs::write(tmp.path().join("go.mod"), "module example.com/x\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: blah\nBuild-Depends: golang-go\nXS-Go-Import-Path: example.com/x\n\nPackage: blah\nArchitecture: any\nDescription: blah\n blah\n",
        );
    }
}
