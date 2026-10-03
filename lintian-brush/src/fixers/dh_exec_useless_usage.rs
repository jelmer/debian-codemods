use crate::debhelper::{
    extract_subst_vars, has_dh_exec_filter, has_dh_exec_rename, is_native_subst_var,
    read_dh_exec_script,
};
use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::Path;

/// Debhelper compat level from which executable config files are run.
const USES_EXECUTABLE_FILES: u8 = 9;

/// Debhelper compat level from which dh_install and friends substitute the
/// common `${DEB_*}` variables themselves.
const DEB_SUBST_LEVEL: u8 = 13;

/// The debhelper config files lintian reports this tag for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    /// Lines are `source... [dest]`.
    Install,
    /// Every word on a line is a manual page to install.
    Manpages,
}

// lintian matches the path against /debian\/.*(install|manpages)/, which is
// looser than this. Only these two categories take wildcards the way the
// rewrite below needs, though.
fn category_for_basename(basename: &str) -> Option<Category> {
    let candidate = match basename.rsplit_once('.') {
        Some((_, ext)) => ext,
        None => basename,
    };
    match candidate {
        "install" => Some(Category::Install),
        "manpages" => Some(Category::Manpages),
        _ => None,
    }
}

/// How certain it is that `token` still matches the same files once its
/// `${VAR}` path components are replaced with `*`.
///
/// Returns `None` if the token contains a substitution that can't be
/// replaced by a wildcard, and `Certainty::Certain` if it has none at all.
fn wildcard_certainty(token: &str) -> Option<Certainty> {
    let mut certainty = Certainty::Certain;
    for component in token.split('/') {
        if let Some(var) = component
            .strip_prefix("${")
            .and_then(|c| c.strip_suffix('}'))
        {
            if !is_native_subst_var(var) {
                return None;
            }
            // Until a later component narrows it down, the wildcard picks
            // up every sibling of the directory the variable stood for.
            certainty = Certainty::Possible;
        } else if component.contains("${") {
            return None;
        } else if certainty == Certainty::Possible
            && component.chars().any(|c| !matches!(c, '*' | '?'))
        {
            certainty = Certainty::Likely;
        }
    }
    Some(certainty)
}

fn replace_substs_with_wildcards(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((before, (_, after))) = rest
        .split_once("${")
        .and_then(|(before, after)| Some((before, after.split_once('}')?)))
    {
        out.push_str(before);
        out.push('*');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Rewrite a single config line so that it no longer relies on dh-exec.
///
/// `native_subst` is whether debhelper substitutes `${DEB_*}` variables
/// itself. Returns `None` if the line can't be rewritten.
fn rewrite_line(line: &str, category: Category, native_subst: bool) -> Option<(String, Certainty)> {
    if native_subst {
        return extract_subst_vars(line)
            .iter()
            .all(|var| is_native_subst_var(var))
            .then(|| (line.to_string(), Certainty::Certain));
    }

    let mut sources: Vec<&str> = line.split_whitespace().collect();
    if category == Category::Install && sources.len() > 1 {
        // A wildcard is meaningless in a destination.
        if sources.pop().is_some_and(|dest| dest.contains("${")) {
            return None;
        }
    }
    let mut certainty = Certainty::Certain;
    for token in sources {
        certainty = certainty.min(wildcard_certainty(token)?);
    }
    Some((replace_substs_with_wildcards(line), certainty))
}

/// Rewrite the body of a dh-exec config file (everything after the hashbang
/// line) so that it works without dh-exec.
///
/// Returns `None` if the file needs dh-exec, or if it isn't clear how to do
/// without.
fn rewrite_body(body: &str, category: Category, native_subst: bool) -> Option<(String, Certainty)> {
    let mut out = String::with_capacity(body.len());
    let mut certainty = Certainty::Certain;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
            continue;
        }
        if has_dh_exec_rename(line) || has_dh_exec_filter(line) {
            return None;
        }
        let (new_line, line_certainty) = rewrite_line(line, category, native_subst)?;
        out.push_str(&new_line);
        certainty = certainty.min(line_certainty);
    }
    Some((out, certainty))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let debian = Path::new("debian");
    let Some(entries) = ws.list_dir(debian)? else {
        return Ok(Vec::new());
    };
    let debhelper_level = debian_workspace::compat_level(ws)?;
    if debhelper_level.is_some_and(|level| level < USES_EXECUTABLE_FILES) {
        return Ok(Vec::new());
    }
    let native_subst = debhelper_level.is_some_and(|level| level >= DEB_SUBST_LEVEL);

    let mut diagnostics = Vec::new();
    for name in entries {
        let Some(category) = category_for_basename(&name) else {
            continue;
        };
        let rel = debian.join(&name);
        let Some((mode, body)) = read_dh_exec_script(ws, &rel)? else {
            continue;
        };
        let Some((new_body, certainty)) = rewrite_body(&body, category, native_subst) else {
            continue;
        };

        let description = if new_body == body {
            format!("Remove unnecessary dh-exec usage from {}.", rel.display())
        } else {
            format!(
                "Use wildcards rather than dh-exec substitutions in {}.",
                rel.display()
            )
        };
        let issue = LintianIssue::source_with_info(
            "dh-exec-useless-usage",
            Visibility::Info,
            vec![format!("[{}]", rel.display())],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                description.clone(),
                description,
                vec![
                    Action::Filesystem(FilesystemAction::Write {
                        file: rel.clone(),
                        content: new_body.into_bytes(),
                    }),
                    Action::Filesystem(FilesystemAction::SetMode {
                        file: rel,
                        mode: mode & !0o111,
                    }),
                ],
            )
            .with_certainty(certainty),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "dh-exec-useless-usage",
    tags: ["dh-exec-useless-usage"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/*.install"),
        debian_workspace::Trigger::Glob("debian/*.manpages"),
        debian_workspace::Trigger::File("debian/install"),
        debian_workspace::Trigger::File("debian/manpages"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        minimum_certainty: Option<Certainty>,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        let preferences = FixerPreferences {
            minimum_certainty,
            ..Default::default()
        };
        DetectorImpl.apply(&ws, &preferences)
    }

    fn write_pkg(base: &Path, name: &str, body: &str, mode: u32, compat: Option<u8>) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        if let Some(level) = compat {
            fs::write(debian.join("compat"), format!("{}\n", level)).unwrap();
        }
    }

    #[test]
    fn test_category_for_basename() {
        assert_eq!(category_for_basename("install"), Some(Category::Install));
        assert_eq!(
            category_for_basename("foo.install"),
            Some(Category::Install)
        );
        assert_eq!(
            category_for_basename("foo.manpages"),
            Some(Category::Manpages)
        );
        assert_eq!(category_for_basename("foo.docs"), None);
        assert_eq!(category_for_basename("install-foo.docs"), None);
        assert_eq!(category_for_basename("rules"), None);
    }

    #[test]
    fn test_wildcard_certainty_no_subst() {
        assert_eq!(wildcard_certainty("usr/bin/foo"), Some(Certainty::Certain));
    }

    #[test]
    fn test_wildcard_certainty_specific_file() {
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_MULTIARCH}/libfoo.so.*"),
            Some(Certainty::Likely)
        );
    }

    #[test]
    fn test_wildcard_certainty_whole_directory() {
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_MULTIARCH}"),
            Some(Certainty::Possible)
        );
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_MULTIARCH}/"),
            Some(Certainty::Possible)
        );
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_MULTIARCH}/*"),
            Some(Certainty::Possible)
        );
    }

    #[test]
    fn test_wildcard_certainty_trailing_subst() {
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_MULTIARCH}/foo/${DEB_HOST_GNU_TYPE}"),
            Some(Certainty::Possible)
        );
    }

    #[test]
    fn test_wildcard_certainty_unknown_variable() {
        assert_eq!(wildcard_certainty("usr/lib/${MY_DIR}/foo"), None);
    }

    #[test]
    fn test_wildcard_certainty_partial_component() {
        assert_eq!(wildcard_certainty("usr/bin/${DEB_HOST_GNU_TYPE}-gcc"), None);
        assert_eq!(
            wildcard_certainty("usr/lib/${DEB_HOST_ARCH}${DEB_HOST_ARCH_OS}/foo"),
            None
        );
    }

    #[test]
    fn test_replace_substs_with_wildcards() {
        assert_eq!(
            replace_substs_with_wildcards(
                "usr/lib/${DEB_HOST_MULTIARCH}/a\tusr/lib/${DEB_HOST_MULTIARCH}/b\n"
            ),
            "usr/lib/*/a\tusr/lib/*/b\n"
        );
        assert_eq!(
            replace_substs_with_wildcards("usr/bin/foo\n"),
            "usr/bin/foo\n"
        );
    }

    #[test]
    fn test_rewrite_line_without_subst() {
        assert_eq!(
            rewrite_line("usr/bin/foo\n", Category::Install, false),
            Some(("usr/bin/foo\n".to_string(), Certainty::Certain))
        );
    }

    #[test]
    fn test_rewrite_line_native_subst() {
        assert_eq!(
            rewrite_line("usr/lib/${DEB_HOST_MULTIARCH}\n", Category::Install, true),
            Some((
                "usr/lib/${DEB_HOST_MULTIARCH}\n".to_string(),
                Certainty::Certain
            ))
        );
        assert_eq!(
            rewrite_line("usr/lib/${MY_DIR}\n", Category::Install, true),
            None
        );
    }

    #[test]
    fn test_rewrite_line_explicit_destination() {
        assert_eq!(
            rewrite_line(
                "usr/lib/${DEB_HOST_MULTIARCH}/foo.so  usr/lib/foo\n",
                Category::Install,
                false
            ),
            Some((
                "usr/lib/*/foo.so  usr/lib/foo\n".to_string(),
                Certainty::Likely
            ))
        );
    }

    #[test]
    fn test_rewrite_line_subst_in_destination() {
        assert_eq!(
            rewrite_line(
                "usr/lib/foo.so usr/lib/${DEB_HOST_MULTIARCH}\n",
                Category::Install,
                false
            ),
            None
        );
    }

    #[test]
    fn test_rewrite_line_manpages_has_no_destination() {
        assert_eq!(
            rewrite_line(
                "doc/foo.1 build/${DEB_HOST_GNU_TYPE}/bar.1\n",
                Category::Manpages,
                false
            ),
            Some(("doc/foo.1 build/*/bar.1\n".to_string(), Certainty::Likely))
        );
    }

    #[test]
    fn test_rewrite_body() {
        assert_eq!(
            rewrite_body(
                "# libraries\nusr/lib/${DEB_HOST_MULTIARCH}/libfoo.so.*\n\nusr/bin/foo\n",
                Category::Install,
                false
            ),
            Some((
                "# libraries\nusr/lib/*/libfoo.so.*\n\nusr/bin/foo\n".to_string(),
                Certainty::Likely
            ))
        );
    }

    #[test]
    fn test_rewrite_body_least_certain_line_wins() {
        assert_eq!(
            rewrite_body(
                "usr/lib/${DEB_HOST_MULTIARCH}\nusr/lib/${DEB_HOST_MULTIARCH}/libfoo.so.*\n",
                Category::Install,
                false
            ),
            Some((
                "usr/lib/*\nusr/lib/*/libfoo.so.*\n".to_string(),
                Certainty::Possible
            ))
        );
    }

    #[test]
    fn test_rewrite_body_needs_dh_exec() {
        assert_eq!(
            rewrite_body("usr/bin/foo [amd64]\n", Category::Install, false),
            None
        );
        assert_eq!(
            rewrite_body("usr/bin/foo <!nodoc>\n", Category::Install, false),
            None
        );
        assert_eq!(
            rewrite_body("foo => usr/bin/bar\n", Category::Install, false),
            None
        );
    }

    #[test]
    fn test_replaces_substitution_with_wildcard() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.install",
            "#!/usr/bin/dh-exec\nusr/lib/${DEB_HOST_MULTIARCH}/libfoo.so.*\n",
            0o755,
            Some(12),
        );
        let result = run_apply(tmp.path(), Some(Certainty::Likely)).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Likely));
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("dh-exec-useless-usage".to_string())
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("[debian/foo.install]".to_string())
        );
        let install = tmp.path().join("debian/foo.install");
        assert_eq!(
            fs::read_to_string(&install).unwrap(),
            "usr/lib/*/libfoo.so.*\n"
        );
        assert_eq!(
            fs::metadata(&install).unwrap().permissions().mode() & 0o111,
            0
        );
    }

    #[test]
    fn test_wildcard_is_not_certain() {
        let tmp = TempDir::new().unwrap();
        let body = "#!/usr/bin/dh-exec\nusr/lib/${DEB_HOST_MULTIARCH}/libfoo.so.*\n";
        write_pkg(tmp.path(), "foo.install", body, 0o755, Some(12));
        let result = run_apply(tmp.path(), Some(Certainty::Certain));
        assert!(
            matches!(result, Err(FixerError::NotCertainEnough(..))),
            "{:?}",
            result
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo.install")).unwrap(),
            body
        );
    }

    #[test]
    fn test_whole_directory_wildcard_is_only_possible() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "install",
            "#! /usr/bin/dh-exec\nusr/lib/${DEB_HOST_MULTIARCH}\n",
            0o755,
            Some(12),
        );
        let result = run_apply(tmp.path(), Some(Certainty::Likely));
        assert!(
            matches!(result, Err(FixerError::NotCertainEnough(..))),
            "{:?}",
            result
        );
        let result = run_apply(tmp.path(), Some(Certainty::Possible)).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Possible));
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/install")).unwrap(),
            "usr/lib/*\n"
        );
    }

    #[test]
    fn test_manpages() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.manpages",
            "#!/usr/bin/dh-exec\nbuild/${DEB_HOST_GNU_TYPE}/foo.1\n",
            0o755,
            Some(12),
        );
        let result = run_apply(tmp.path(), Some(Certainty::Likely)).unwrap();
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("[debian/foo.manpages]".to_string())
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo.manpages")).unwrap(),
            "build/*/foo.1\n"
        );
    }

    #[test]
    fn test_ignores_other_config_files() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.docs",
            "#!/usr/bin/dh-exec\nREADME\n",
            0o755,
            Some(12),
        );
        assert!(matches!(
            run_apply(tmp.path(), Some(Certainty::Possible)),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_ignores_non_executable() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.install",
            "#!/usr/bin/dh-exec\nusr/bin/foo\n",
            0o644,
            Some(12),
        );
        assert!(matches!(
            run_apply(tmp.path(), Some(Certainty::Possible)),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_ignores_other_scripts() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.install",
            "#!/bin/sh\necho usr/bin/foo\n",
            0o755,
            Some(12),
        );
        assert!(matches!(
            run_apply(tmp.path(), Some(Certainty::Possible)),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_ignores_compat_without_executable_config_files() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "foo.install",
            "#!/usr/bin/dh-exec\nusr/bin/foo\n",
            0o755,
            Some(8),
        );
        assert!(matches!(
            run_apply(tmp.path(), Some(Certainty::Possible)),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(tmp.path(), Some(Certainty::Possible)),
            Err(FixerError::NoChanges)
        ));
    }
}
