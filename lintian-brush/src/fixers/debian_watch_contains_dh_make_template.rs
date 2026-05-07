use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

const DH_MAKE_TEMPLATE: &str = r"s/.+\/v?(\d\S+)\.tar\.gz/<project>-$1\.tar\.gz/";

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

    let mut diagnostics = Vec::new();
    for entry in watch_file.entries() {
        let Some(filenamemangle) = entry.get_option("filenamemangle") else {
            continue;
        };
        if filenamemangle != DH_MAKE_TEMPLATE {
            continue;
        }
        let issue = LintianIssue::source_with_info(
            "debian-watch-contains-dh_make-template",
            vec![format!("{} [debian/watch]", filenamemangle)],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Remove dh_make template from debian watch.",
                vec![Action::Watch(WatchAction::RemoveEntryOption {
                    file: watch_rel.clone(),
                    url: entry.url(),
                    option: "filenamemangle".into(),
                })],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-contains-dh_make-template",
    tags: ["debian-watch-contains-dh_make-template"],
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

    #[test]
    fn test_removes_dh_make_template() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "version=2\nopts=filenamemangle=s/.+\\/v?(\\d\\S+)\\.tar\\.gz/<project>-$1\\.tar\\.gz/ https://github.com/example/project/releases .*\\/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Remove dh_make template from debian watch."
        );
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "version=2\nhttps://github.com/example/project/releases .*\\/v?(\\d\\S+)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_template_pattern() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/example/project/releases .*/v?(\\d\\S+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
