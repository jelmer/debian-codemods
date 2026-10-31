use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Rewrite `.dfsg` to `+dfsg` in a dversionmangle substitution, where it
/// appears as a *literal* (escaped) period before `dfsg`. Returns `None` if
/// nothing to change.
///
/// We only touch forms the maintainer wrote on purpose: `\.dfsg`, `[.]dfsg`,
/// `\.dfsg` inside a character class, and literal `.dfsg` where the period is
/// part of a longer regex (`.dfsg` can also match any char + `dfsg`, but in
/// practice it's a period in dversionmangle). We rewrite the escaped forms;
/// bare `.dfsg` is left alone since changing it could alter regex semantics.
fn rewrite_dversionmangle(current: &str) -> Option<String> {
    let patterns: &[(&str, &str)] = &[(r"\.dfsg", r"\+dfsg"), ("[.]dfsg", "[+]dfsg")];
    let mut out = current.to_string();
    let mut changed = false;
    for (from, to) in patterns {
        if out.contains(from) {
            out = out.replace(from, to);
            changed = true;
        }
    }
    if changed {
        Some(out)
    } else {
        None
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(version) = ws.current_version() else {
        return Ok(Vec::new());
    };
    if version.is_native() {
        return Ok(Vec::new());
    }
    let version_str = version.to_string();
    if !version_str.contains(".dfsg") {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "dfsg-version-with-period",
        Visibility::Info,
        vec![version_str.clone()],
    );

    let watch_rel = PathBuf::from("debian/watch");
    let mut actions: Vec<Action> = Vec::new();
    match ws.parsed_watch() {
        Ok(watch_file) => {
            for entry in watch_file.entries() {
                let Some(current) = entry.get_option("dversionmangle") else {
                    continue;
                };
                let Some(new_value) = rewrite_dversionmangle(&current) else {
                    continue;
                };
                actions.push(Action::Watch(WatchAction::SetEntryOption {
                    file: watch_rel.clone(),
                    url: entry.url(),
                    option: "dversionmangle".into(),
                    value: new_value,
                }));
            }
        }
        Err(debian_workspace::Error::NotFound) => {}
        Err(e) => return Err(e.into()),
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!("Version {} uses `.dfsg` separator.", version_str),
        "Use `+dfsg` instead of `.dfsg` in debian/watch dversionmangle.",
        actions,
    )
    .with_certainty(Certainty::Confident)])
}

declare_detector! {
    name: "dfsg-version-with-period",
    tags: ["dfsg-version-with-period"],
    triggers: [
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Version),
        debian_workspace::Trigger::Watch(debian_workspace::WatchAspect::Option(
            "dversionmangle",
        )),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn rewrite_escaped_period() {
        assert_eq!(
            rewrite_dversionmangle(r"s/\.dfsg\d*$//").as_deref(),
            Some(r"s/\+dfsg\d*$//"),
        );
    }

    #[test]
    fn rewrite_character_class_period() {
        assert_eq!(
            rewrite_dversionmangle("s/[.]dfsg//").as_deref(),
            Some("s/[+]dfsg//"),
        );
    }

    #[test]
    fn rewrite_leaves_unrelated_mangle_alone() {
        assert_eq!(rewrite_dversionmangle("s/-.*$//"), None);
    }

    #[test]
    fn rewrite_leaves_plus_dfsg_alone() {
        assert_eq!(rewrite_dversionmangle(r"s/\+dfsg\d*$//"), None);
    }

    #[test]
    fn rewrite_leaves_bare_dot_alone() {
        // A bare `.dfsg` could match any character before `dfsg`; we don't
        // know whether the maintainer meant a literal period, so we leave
        // it to avoid semantic drift.
        assert_eq!(rewrite_dversionmangle("s/.dfsg//"), None);
    }

    fn run_apply(base: &Path, version: &str) -> Result<crate::FixerResult, FixerError> {
        let v: Version = version.parse().unwrap();
        let adapter = DetectorImpl;
        let ws =
            debian_workspace::fs_workspace::FsWorkspace::new(base, Some("test".into()), Some(v));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_changelog(base: &Path, version: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            format!(
                "example ({}) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
                version,
            ),
        )
        .unwrap();
    }

    #[test]
    fn fixes_dversionmangle() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0.dfsg1-1");
        let watch = tmp.path().join("debian/watch");
        fs::write(
            &watch,
            "version=4\nopts=dversionmangle=s/\\.dfsg\\d*$// https://example.com/project .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path(), "1.0.dfsg1-1").unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=4\nopts=dversionmangle=s/\\+dfsg\\d*$// https://example.com/project .*/v?(\\d\\S+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn skips_native_package() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0.dfsg1");
        fs::write(
            tmp.path().join("debian/watch"),
            "version=4\nopts=dversionmangle=s/\\.dfsg\\d*$// https://example.com/project .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "1.0.dfsg1"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn skips_version_without_dfsg_period() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0+dfsg1-1");
        fs::write(
            tmp.path().join("debian/watch"),
            "version=4\nopts=dversionmangle=s/\\+dfsg\\d*$// https://example.com/project .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "1.0+dfsg1-1"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn skips_when_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0.dfsg1-1");
        assert!(matches!(
            run_apply(tmp.path(), "1.0.dfsg1-1"),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn skips_when_watch_mangle_already_plus() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0.dfsg1-1");
        fs::write(
            tmp.path().join("debian/watch"),
            "version=4\nopts=dversionmangle=s/\\+dfsg\\d*$// https://example.com/project .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(
            run_apply(tmp.path(), "1.0.dfsg1-1"),
            Err(FixerError::NoChanges)
        ));
    }
}
