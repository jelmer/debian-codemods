use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction, TextRange};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

/// A `debian/install` line that installs a file straight into a
/// `modprobe.d` directory, which `dh_installmodules` should handle instead.
struct ModprobeInstall {
    /// 1-based line number, matching the position in lintian's pointer.
    position: usize,
    /// Byte range of the whole line, including its trailing newline.
    range: TextRange,
    /// Source path as written on the line (e.g. `debian/foo.conf`).
    source: String,
}

/// Match lintian's `dh-install-instead-of-dh-installmodules` regex against a
/// raw install-file line, returning the source field.
///
/// Lintian applies `^debian\/.+[[:blank:]]+.+\/modprobe\.d.?$` to every line
/// of `debian/install` and `debian/*.install`. We reproduce that boolean
/// match and, on success, return the first whitespace-delimited token (the
/// source path being installed).
fn match_modprobe_line(line: &str) -> Option<&str> {
    if !line.starts_with("debian/") {
        return None;
    }
    // The line must end with `/modprobe.d` followed by at most one arbitrary
    // character (the `.?` at the end of the regex): `/modprobe.d`,
    // `/modprobe.d/`, `/modprobe.dX`. A longer tail (a trailing filename)
    // is not matched by lintian, so the destination is always a directory.
    let ends_at_modprobe_dir = line.ends_with("/modprobe.d")
        || line
            .char_indices()
            .next_back()
            .map(|(i, _)| line[..i].ends_with("/modprobe.d"))
            .unwrap_or(false);
    if !ends_at_modprobe_dir {
        return None;
    }

    // Source, blanks, destination: there must be a whitespace-separated
    // token before the destination.
    let idx = line.find([' ', '\t'])?;
    let source = &line[..idx];
    let rest = line[idx..].trim_start_matches([' ', '\t']);
    if source.is_empty() || rest.is_empty() {
        return None;
    }
    Some(source)
}

/// Resolve the binary package that an install file belongs to.
///
/// `debian/<pkg>.install` belongs to `<pkg>`; the unprefixed
/// `debian/install` belongs to the first binary package in `debian/control`.
fn install_file_package(ws: &dyn Workspace, basename: &str) -> Option<String> {
    if let Some(pkg) = basename.strip_suffix(".install") {
        return Some(pkg.to_string());
    }
    if basename == "install" {
        let control = ws.parsed_control().ok()?;
        return control.binaries().next().and_then(|b| b.name());
    }
    None
}

/// Names of every `debian/install` / `debian/*.install` file in the tree.
fn install_files(ws: &dyn Workspace) -> Result<Vec<String>, FixerError> {
    let Some(entries) = ws.list_dir(Path::new("debian"))? else {
        return Ok(Vec::new());
    };
    Ok(entries
        .into_iter()
        .filter(|name| name == "install" || name.ends_with(".install"))
        .collect())
}

/// Compute the `debian/<...>.modprobe` name that reproduces the destination
/// filename `dh_install` currently produces.
///
/// `dh_installmodules` installs `debian/package.modprobe` as
/// `modprobe.d/package.conf` and `debian/package.name.modprobe` as
/// `modprobe.d/name.conf`. The current install line installs
/// `debian/<conf_stem>.conf` into the directory, so the destination filename
/// is `<conf_stem>.conf`; we pick the `.modprobe` name that yields the same.
fn modprobe_target(package: &str, conf_stem: &str) -> String {
    if conf_stem == package {
        format!("debian/{}.modprobe", package)
    } else {
        format!("debian/{}.{}.modprobe", package, conf_stem)
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for basename in install_files(ws)? {
        let rel = PathBuf::from("debian").join(&basename);
        let Some(content) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&content) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().into_owned();

        // Collect the matching lines first so several lines that install the
        // same source file can be fixed as a single rename.
        let mut matches: Vec<ModprobeInstall> = Vec::new();
        let mut offset = 0usize;
        for (idx, line) in text.split_inclusive('\n').enumerate() {
            let line_start = offset;
            offset += line.len();
            let body = line.trim_end_matches(['\r', '\n']);
            let Some(source) = match_modprobe_line(body) else {
                continue;
            };
            matches.push(ModprobeInstall {
                position: idx + 1,
                range: TextRange {
                    start: line_start,
                    end: offset,
                },
                source: source.to_string(),
            });
        }

        if matches.is_empty() {
            continue;
        }

        let package = install_file_package(ws, &basename);

        // Group matching lines by source file: each group renames one source
        // file and drops all its install lines.
        let mut sources: Vec<String> = Vec::new();
        for m in &matches {
            if !sources.contains(&m.source) {
                sources.push(m.source.clone());
            }
        }

        // If every non-blank line of the install file is a modprobe line and
        // every group can be fixed, the file is left empty by the fix, so we
        // remove it outright rather than leaving an empty file behind. This is
        // only safe when all groups are fixable; otherwise removing the file
        // would drop an install line we aren't rewriting.
        let only_modprobe_lines =
            text.lines().filter(|l| !l.trim().is_empty()).count() == matches.len();
        let all_fixable = sources.iter().all(|source| {
            let group: Vec<&ModprobeInstall> =
                matches.iter().filter(|m| &m.source == source).collect();
            build_actions(ws, &rel, &package, source, &group, false).is_some()
        });

        // Only the first group carries the whole-file delete, so the file
        // isn't deleted more than once.
        let mut delete_pending = only_modprobe_lines && all_fixable;

        for source in sources {
            let group: Vec<&ModprobeInstall> =
                matches.iter().filter(|m| m.source == source).collect();

            // lintian emits one hint per matching line, all describing the
            // same underlying problem for this source file. We report it as a
            // single issue anchored on the first line and fix the whole group
            // as one rename plus line removals.
            let issue = LintianIssue::source_with_info(
                "dh-install-instead-of-dh-installmodules",
                Visibility::Info,
                vec![format!("[{}:{}]", rel_str, group[0].position)],
            );
            let message = format!(
                "Install {} via dh_installmodules instead of dh_install.",
                source
            );

            match build_actions(ws, &rel, &package, &source, &group, delete_pending) {
                Some((rename_to, mut acts)) => {
                    // The whole-file delete only rides along with the first
                    // fixable group.
                    delete_pending = false;
                    // Drop the install lines highest-first so earlier removals
                    // do not shift the byte offsets of later ones.
                    acts.sort_by_key(|a| std::cmp::Reverse(action_start(a)));
                    diagnostics.push(
                        Diagnostic::with_plans(
                            issue,
                            message,
                            vec![ActionPlan {
                                label: format!("Rename {} to {}.", source, rename_to),
                                opinionated: false,
                                certainty: Some(Certainty::Certain),
                                actions: acts,
                            }],
                        )
                        .with_certainty(Certainty::Certain),
                    );
                }
                None => {
                    // We can't safely rename (glob, non-conf name, unknown
                    // package, subdirectory, existing target). Report the
                    // issue without a fix.
                    diagnostics.push(
                        Diagnostic::with_plans(issue, message, Vec::new())
                            .with_certainty(Certainty::Certain),
                    );
                }
            }
        }
    }

    Ok(diagnostics)
}

/// Byte offset an action starts at, for ordering line removals back-to-front.
/// Non-`ReplaceText` actions (the rename) sort last (offset 0).
fn action_start(action: &Action) -> usize {
    match action {
        Action::Filesystem(FilesystemAction::ReplaceText { range, .. }) => range.start,
        _ => 0,
    }
}

/// Build the rename + line-removal actions for one source file, or `None`
/// when a correct rename can't be determined.
fn build_actions(
    ws: &dyn Workspace,
    install_file: &Path,
    package: &Option<String>,
    source: &str,
    group: &[&ModprobeInstall],
    delete_file: bool,
) -> Option<(String, Vec<Action>)> {
    let package = package.as_ref()?;

    // The source must be a plain file under debian/ with a `.conf` name; a
    // glob or a nested path is not something we can rename unambiguously.
    let source_path = Path::new(source);
    let rel_source = source_path.strip_prefix("debian").ok()?;
    if rel_source.components().count() != 1 {
        return None;
    }
    let file_name = rel_source.file_name()?.to_str()?;
    if file_name.contains(['*', '?', '[']) {
        return None;
    }
    let conf_stem = file_name.strip_suffix(".conf")?;
    if conf_stem.is_empty() {
        return None;
    }

    // The source file has to actually exist for the rename to make sense.
    ws.read_file(source_path).ok().flatten()?;

    let target = modprobe_target(package, conf_stem);
    // Don't clobber an existing target file.
    if !matches!(ws.read_file(Path::new(&target)), Ok(None)) {
        return None;
    }

    let mut actions = vec![Action::Filesystem(FilesystemAction::Rename {
        file: PathBuf::from(source),
        to: PathBuf::from(&target),
    })];
    if delete_file {
        // The install file has nothing left after this group's lines are
        // removed, so drop the whole file rather than leaving it empty.
        actions.push(Action::Filesystem(FilesystemAction::Delete {
            file: install_file.to_path_buf(),
        }));
    } else {
        for m in group {
            actions.push(Action::Filesystem(FilesystemAction::ReplaceText {
                file: install_file.to_path_buf(),
                range: m.range.clone(),
                replacement: String::new(),
            }));
        }
    }
    Some((target, actions))
}

fn describe(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    if fixed.len() == 1 {
        "Install modprobe.d file via dh_installmodules instead of dh_install.".to_string()
    } else {
        format!(
            "Install {} modprobe.d files via dh_installmodules instead of dh_install.",
            fixed.len()
        )
    }
}

declare_detector! {
    name: "dh-install-instead-of-dh-installmodules",
    tags: ["dh-install-instead-of-dh-installmodules"],
    triggers: [
        debian_workspace::Trigger::File("debian/install"),
        debian_workspace::Trigger::Glob("debian/*.install"),
    ],
    cost: crate::detector::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const CONTROL: &str = "\
Source: foo
Maintainer: Jane Doe <jane@example.com>

Package: foo
Architecture: any
Description: test
 long
";

    fn write(base: &Path, rel: &str, content: &str) {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("foo".into()),
            Some(version),
        );
        DetectorImpl.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn test_match_modprobe_line() {
        assert_eq!(
            match_modprobe_line("debian/foo.conf etc/modprobe.d/"),
            Some("debian/foo.conf")
        );
        assert_eq!(
            match_modprobe_line("debian/foo.conf\tusr/lib/modprobe.d/"),
            Some("debian/foo.conf")
        );
        assert_eq!(
            match_modprobe_line("debian/foo.conf etc/modprobe.d"),
            Some("debian/foo.conf")
        );
    }

    #[test]
    fn test_no_match() {
        // Not under debian/.
        assert_eq!(match_modprobe_line("foo.conf etc/modprobe.d/"), None);
        // Comment.
        assert_eq!(
            match_modprobe_line("# debian/foo.conf etc/modprobe.d/"),
            None
        );
        // No destination.
        assert_eq!(match_modprobe_line("debian/foo.conf"), None);
        // Destination is a file inside modprobe.d, not the directory.
        assert_eq!(
            match_modprobe_line("debian/foo.conf etc/modprobe.d/foo.conf"),
            None
        );
        // Unrelated directory.
        assert_eq!(match_modprobe_line("debian/foo.conf etc/default/"), None);
    }

    #[test]
    fn test_modprobe_target() {
        assert_eq!(modprobe_target("foo", "foo"), "debian/foo.modprobe");
        assert_eq!(
            modprobe_target("foo", "blacklist"),
            "debian/foo.blacklist.modprobe"
        );
    }

    #[test]
    fn test_no_install_file() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unrelated_install_line_left_alone() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/foo.conf etc/default/\n",
        );
        write(tmp.path(), "debian/foo.conf", "options foo\n");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_renames_and_drops_line() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/blacklist.conf etc/modprobe.d/\n",
        );
        write(tmp.path(), "debian/blacklist.conf", "blacklist foo\n");

        let result = run_apply(tmp.path()).unwrap();
        assert!(!tmp.path().join("debian/blacklist.conf").exists());
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo.blacklist.modprobe")).unwrap(),
            "blacklist foo\n"
        );
        // The install file is now empty and removed.
        assert!(!tmp.path().join("debian/install").exists());
        assert_eq!(result.fixed_lintian_issues.len(), 1);
    }

    #[test]
    fn test_conf_matches_package_name() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/foo.conf usr/lib/modprobe.d/\n",
        );
        write(tmp.path(), "debian/foo.conf", "options foo\n");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo.modprobe")).unwrap(),
            "options foo\n"
        );
    }

    #[test]
    fn test_keeps_other_install_lines() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/other usr/share/foo/\ndebian/blacklist.conf etc/modprobe.d/\n",
        );
        write(tmp.path(), "debian/blacklist.conf", "blacklist foo\n");
        write(tmp.path(), "debian/other", "x\n");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/install")).unwrap(),
            "debian/other usr/share/foo/\n"
        );
    }

    #[test]
    fn test_same_file_two_destinations() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/test.conf etc/modprobe.d/\ndebian/test.conf usr/lib/modprobe.d/\n",
        );
        write(tmp.path(), "debian/test.conf", "# empty\n");

        run_apply(tmp.path()).unwrap();
        assert!(!tmp.path().join("debian/install").exists());
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo.test.modprobe")).unwrap(),
            "# empty\n"
        );
    }

    #[test]
    fn test_named_install_file() {
        let tmp = TempDir::new().unwrap();
        let control = CONTROL.replace("Package: foo", "Package: foo-modules");
        write(tmp.path(), "debian/control", &control);
        write(
            tmp.path(),
            "debian/foo-modules.install",
            "debian/blacklist.conf etc/modprobe.d/\n",
        );
        write(tmp.path(), "debian/blacklist.conf", "blacklist foo\n");

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/foo-modules.blacklist.modprobe")).unwrap(),
            "blacklist foo\n"
        );
    }

    #[test]
    fn test_glob_source_reported_without_fix() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/*.conf etc/modprobe.d/\n",
        );

        // The line matches the tag but we can't rename a glob; report only.
        let version: Version = "1.0".parse().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(version),
        );
        let diagnostics = detect(&ws, &FixerPreferences::default()).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].plans.is_empty());
        // Nothing is changed on apply.
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_missing_source_file_not_fixed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/gone.conf etc/modprobe.d/\n",
        );
        // debian/gone.conf does not exist.
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_target_exists_not_fixed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "debian/control", CONTROL);
        write(
            tmp.path(),
            "debian/install",
            "debian/foo.conf etc/modprobe.d/\n",
        );
        write(tmp.path(), "debian/foo.conf", "options foo\n");
        write(tmp.path(), "debian/foo.modprobe", "already here\n");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
