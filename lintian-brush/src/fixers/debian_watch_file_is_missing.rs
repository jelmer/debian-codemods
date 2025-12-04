use crate::{declare_fixer, Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_watch::{Entry, WatchFile};
use debversion::Version;
use std::path::Path;
use std::process::Command;

struct WatchCandidate {
    entry: Entry,
    site: String,
    certainty: Option<Certainty>,
    preference: i32,
}

/// Find watch file candidates for a package
fn find_candidates(
    path: &Path,
    _good_upstream_versions: &[String],
    _net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();

    // Check for setup.py (PyPI packages)
    let setup_py = path.join("setup.py");
    if setup_py.exists() {
        if let Ok(Some(candidate)) = candidates_from_setup_py(&setup_py) {
            candidates.push(candidate);
        }
    }

    // Check for debian/upstream/metadata
    let upstream_metadata = path.join("debian/upstream/metadata");
    if upstream_metadata.exists() {
        if let Ok(mut cands) = candidates_from_upstream_metadata(&upstream_metadata) {
            candidates.append(&mut cands);
        }
    }

    // Sort by certainty (descending) and preference (descending)
    candidates.sort_by(|a, b| {
        let a_conf = certainty_to_confidence(a.certainty.as_ref());
        let b_conf = certainty_to_confidence(b.certainty.as_ref());
        b_conf
            .cmp(&a_conf)
            .then_with(|| b.preference.cmp(&a.preference))
    });

    Ok(candidates)
}

fn certainty_to_confidence(certainty: Option<&Certainty>) -> i32 {
    match certainty {
        Some(Certainty::Certain) => 3,
        Some(Certainty::Confident) => 2,
        Some(Certainty::Likely) => 1,
        Some(Certainty::Possible) => 0,
        None => 1, // default to likely
    }
}

/// Extract watch candidates from setup.py (PyPI packages)
fn candidates_from_setup_py(
    path: &Path,
) -> Result<Option<WatchCandidate>, Box<dyn std::error::Error>> {
    // Use Python to extract project name from setup.py
    let script = r#"
import sys
import os
sys.path.insert(0, os.path.dirname(sys.argv[1]))
try:
    from setuptools import setup
except ImportError:
    pass
from distutils.core import run_setup
try:
    result = run_setup(sys.argv[1], stop_after='config')
    name = result.get_name()
    if name:
        print(name)
except:
    pass
"#;

    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(path.as_os_str())
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let project = stdout.trim();

    if project.is_empty() {
        return Ok(None);
    }

    // Create watch entry for PyPI
    let filename_regex = format!(
        r"{}-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))",
        regex::escape(project)
    );
    let url = format!("https://pypi.debian.net/{}/{}", project, filename_regex);

    // Parse as a temporary watch file to extract the entry
    let watch_content = format!("version=4\n{}\n", url);
    let parsed: WatchFile = watch_content.parse()?;
    let entry = parsed.entries().next().ok_or("No entry found")?;

    Ok(Some(WatchCandidate {
        entry,
        site: "pypi".to_string(),
        certainty: Some(Certainty::Likely),
        preference: 1,
    }))
}

/// Extract watch candidates from debian/upstream/metadata
fn candidates_from_upstream_metadata(
    path: &Path,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    use serde_yaml::Value;
    use std::fs;

    let content = fs::read_to_string(path)?;
    let yaml: Value = serde_yaml::from_str(&content)?;

    let mut candidates = Vec::new();

    // Check for CRAN packages
    if let Some(archive) = yaml.get("Archive").and_then(|v| v.as_str()) {
        if archive == "CRAN" {
            if let Some(name) = yaml.get("Name").and_then(|v| v.as_str()) {
                let url = format!(
                    "https://cran.r-project.org/src/contrib/{}_([-.\\d]*)\\.tar\\.gz",
                    name
                );
                let watch_content = format!("version=4\n{}\n", url);
                let parsed: WatchFile = watch_content.parse()?;
                let entry = parsed.entries().next().ok_or("No entry found")?;

                candidates.push(WatchCandidate {
                    entry,
                    site: "cran".to_string(),
                    certainty: Some(Certainty::Likely),
                    preference: 0,
                });
            }
        }
    }

    Ok(candidates)
}

pub fn run(
    base_path: &Path,
    _package: &str,
    version: &Version,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    // Check if watch file already exists
    let watch_path = base_path.join("debian/watch");
    if watch_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check if this issue should be fixed
    let issue = LintianIssue {
        package: None,
        package_type: Some(crate::PackageType::Source),
        tag: Some("debian-watch-file-is-missing".to_string()),
        info: None,
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChanges);
    }

    // Get upstream version
    let upstream_version = version.upstream_version.to_string();

    // Find candidates
    let candidates = find_candidates(
        base_path,
        &[upstream_version],
        preferences.net_access.unwrap_or(false),
    )
    .map_err(|e| FixerError::Other(format!("Failed to find candidates: {}", e)))?;

    if candidates.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Take the best candidate (first after sorting)
    let winner = candidates.into_iter().next().unwrap();

    // Create a new watch file with the entry
    let mut watch_file = WatchFile::new(Some(4));
    watch_file.add_entry(winner.entry);

    // Write the watch file
    std::fs::write(&watch_path, watch_file.to_string())?;

    let mut result = FixerResult::builder(format!("Add debian/watch file, using {}.", winner.site));

    if let Some(certainty) = winner.certainty {
        result = result.certainty(certainty);
    }

    result = result.fixed_tags(vec!["debian-watch-file-is-missing"]);

    Ok(result.build())
}

declare_fixer! {
    name: "debian-watch-file-is-missing",
    tags: ["debian-watch-file-is-missing"],
    apply: |basedir, package, version, preferences| {
        // Native packages don't need watch files
        if version.is_native() {
            return Err(FixerError::NoChanges);
        }
        run(basedir, package, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use debversion::Version;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_skips_if_watch_exists() {
        let dir = TempDir::new().unwrap();
        let debian_dir = dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        fs::write(debian_dir.join("watch"), "version=4\n").unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();

        let result = run(dir.path(), "testpkg", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_find_candidates_from_setup_py() {
        let dir = TempDir::new().unwrap();
        let setup_py = dir.path().join("setup.py");
        fs::write(
            &setup_py,
            r#"#!/usr/bin/python
from distutils.core import setup
setup(name="xandikos", version="42.0")
"#,
        )
        .unwrap();

        let candidates = find_candidates(dir.path(), &[], false).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].site, "pypi");
        assert!(candidates[0]
            .entry
            .url()
            .contains("pypi.debian.net/xandikos"));
    }

    #[test]
    fn test_find_candidates_from_cran() {
        let dir = TempDir::new().unwrap();
        let upstream_dir = dir.path().join("debian/upstream");
        fs::create_dir_all(&upstream_dir).unwrap();
        fs::write(
            upstream_dir.join("metadata"),
            "---\nArchive: CRAN\nName: gower\n",
        )
        .unwrap();

        let candidates = find_candidates(dir.path(), &[], false).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].site, "cran");
        assert!(candidates[0]
            .entry
            .url()
            .contains("cran.r-project.org/src/contrib/gower"));
    }

    #[test]
    fn test_returns_no_changes_when_no_candidates() {
        let dir = TempDir::new().unwrap();
        let debian_dir = dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let version: Version = "1.0-1".parse().unwrap();
        let preferences = FixerPreferences::default();

        let result = run(dir.path(), "testpkg", &version, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
