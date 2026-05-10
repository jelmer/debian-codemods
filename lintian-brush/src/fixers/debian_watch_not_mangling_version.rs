use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;

const REPACK_REGEX: &str = r"(dfsg|debian|ds|repack)";
const DVERSIONMANGLE: &str = r"s/\+(dfsg|ds|debian|repack)(\d*)$//";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let watch_rel = PathBuf::from("debian/watch");
    let watch_file = match ws.parsed_watch() {
        Ok(w) => w,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(first_entry) = changelog.iter().next() else {
        return Ok(Vec::new());
    };
    let Some(version) = first_entry.version() else {
        return Ok(Vec::new());
    };
    let regex = regex::Regex::new(REPACK_REGEX).unwrap();
    if !regex.is_match(&version.to_string()) {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for entry in watch_file.entries() {
        if entry.get_option("dversionmangle").is_some()
            || entry.get_option("uversionmangle").is_some()
        {
            continue;
        }
        let line_number = entry.line() + 1;
        let issue = LintianIssue::source_with_info(
            "debian-watch-not-mangling-version",
            Visibility::Warning,
            vec![format!("{} [debian/watch]", line_number)],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/watch is not mangling repack version suffix.",
                "Add dversionmangle for repack versioning in debian/watch.",
                vec![Action::Watch(WatchAction::SetEntryOption {
                    file: watch_rel.clone(),
                    url: entry.url(),
                    option: "dversionmangle".into(),
                    value: DVERSIONMANGLE.into(),
                })],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-not-mangling-version",
    tags: ["debian-watch-not-mangling-version", "debian-watch-file-should-mangle-version"],
    triggers: [
        crate::workspace::Trigger::Watch(crate::workspace::WatchAspect::Option(
            "dversionmangle",
        )),
        crate::workspace::Trigger::Watch(crate::workspace::WatchAspect::Option(
            "uversionmangle",
        )),
        crate::workspace::Trigger::Changelog(crate::workspace::ChangelogAspect::Version),
    ],
    detect: |ws, prefs| detect(ws, prefs),
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

    fn write_changelog(base: &Path, version: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("changelog"),
            format!(
                "example ({}) unstable; urgency=medium\n\n  * Initial release.\n\n -- Maintainer <maint@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
                version
            ),
        )
        .unwrap();
    }

    #[test]
    fn test_adds_dversionmangle() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0+dfsg-1");
        let watch = tmp.path().join("debian/watch");
        fs::write(
            &watch,
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=4\nopts=dversionmangle=s/\\+(dfsg|ds|debian|repack)(\\d*)$// https://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_version_without_repack() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0-1");
        fs::write(
            tmp.path().join("debian/watch"),
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_has_dversionmangle() {
        let tmp = TempDir::new().unwrap();
        write_changelog(tmp.path(), "1.0+dfsg-1");
        fs::write(
            tmp.path().join("debian/watch"),
            "version=4\nopts=dversionmangle=s/\\+dfsg$// https://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
