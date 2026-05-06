use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, WatchAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences};
use std::path::PathBuf;

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

    if watch_file.version() != 5 {
        return Ok(Vec::new());
    }
    let debian_watch::parse::ParsedWatchFile::Deb822(v5_file) = watch_file else {
        return Ok(Vec::new());
    };

    let mut converted: Vec<&'static str> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for mut entry in v5_file.entries() {
        if entry.as_deb822().get("Template").is_some() {
            continue;
        }
        let url = entry.url();
        // Probe by attempting the conversion on a clone — the result
        // tells us which template (if any) matches. We then emit an
        // action that re-runs the conversion at apply time.
        let Some(template) = entry.try_convert_to_template() else {
            continue;
        };
        let name = match template {
            debian_watch::templates::Template::GitHub { .. } => "GitHub",
            debian_watch::templates::Template::GitLab { .. } => "GitLab",
            debian_watch::templates::Template::PyPI { .. } => "PyPI",
            debian_watch::templates::Template::Npmregistry { .. } => "Npmregistry",
            debian_watch::templates::Template::Metacpan { .. } => "Metacpan",
            debian_watch::templates::Template::Cran { .. } => "CRAN",
            debian_watch::templates::Template::Bioconductor { .. } => "Bioconductor",
        };
        converted.push(name);
        diagnostics.push(
            Diagnostic::untagged(
                String::new(),
                vec![Action::Watch(WatchAction::ConvertEntryToTemplate {
                    file: watch_rel.clone(),
                    url,
                })],
            )
            .with_certainty(Certainty::Confident),
        );
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    let summary = if converted.len() == 1 {
        format!(
            "Use {} template in watch file instead of explicit Source/Matching-Pattern.",
            converted[0]
        )
    } else {
        "Use templates in watch file instead of explicit Source/Matching-Pattern.".to_string()
    };
    for d in &mut diagnostics {
        d.message = summary.clone();
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "debian-watch-use-templates",
    tags: [],
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
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_convert_metacpan_to_template() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "Version: 5\n\nSource: https://cpan.metacpan.org/authors/id/\nMatching-Pattern: .*/Mail-AuthenticationResults@ANY_VERSION@@ARCHIVE_EXT@\nSearchmode: plain\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "Version: 5\n\nTemplate: Metacpan\nDist: Mail-AuthenticationResults\n",
        );
    }

    #[test]
    fn test_convert_github_to_template() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let watch = debian.join("watch");
        fs::write(
            &watch,
            "Version: 5\n\nSource: https://github.com/torvalds/linux/tags\nMatching-Pattern: .*/(?:refs/tags/)?v?@ANY_VERSION@@ARCHIVE_EXT@\nSearchmode: html\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&watch).unwrap(),
            "Version: 5\n\nTemplate: GitHub\nOwner: torvalds\nProject: linux\n",
        );
    }

    #[test]
    fn test_no_change_when_already_using_template() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "Version: 5\n\nTemplate: Metacpan\nDist: Mail-AuthenticationResults\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_v5() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/torvalds/linux/tags .*/v?([\\d.]+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_template_matches() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "Version: 5\n\nSource: https://example.com/downloads/\nMatching-Pattern: .*/v?(\\d+\\.\\d+)\\.tar\\.gz\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_watch_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
