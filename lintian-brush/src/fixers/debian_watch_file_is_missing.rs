use crate::{Certainty, FixerError, FixerPreferences, FixerResult, LintianIssue};
use breezyshim::branch::Branch;
use debversion::Version;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use url::Url;

struct WatchCandidate {
    source: String,
    matching_pattern: String,
    options: Vec<debian_watch::WatchOption>,
    site: String,
    certainty: Option<Certainty>,
    preference: i32,
}

/// Default timeout for HTTP requests (3 seconds)
const DEFAULT_URLLIB_TIMEOUT: Duration = Duration::from_secs(3);

/// User agent for HTTP requests
fn user_agent() -> String {
    format!("lintian-brush/{}", env!("CARGO_PKG_VERSION"))
}

/// Load JSON from a URL
fn load_json(url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(user_agent())
        .timeout(DEFAULT_URLLIB_TIMEOUT)
        .build()?;

    let response = client
        .get(url)
        .header("Accept", "application/json")
        .send()?;

    if response.status() == 404 {
        return Err("Not found".into());
    }

    let json = response.json()?;
    Ok(json)
}

/// Find watch file candidates for a package
fn find_candidates(
    path: &Path,
    good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();

    // Check for setup.py (PyPI packages)
    let setup_py = path.join("setup.py");
    if setup_py.exists() {
        if let Ok(Some(candidate)) =
            candidates_from_setup_py(&setup_py, good_upstream_versions, net_access)
        {
            candidates.push(candidate);
        }
    }

    // Check for debian/upstream/metadata
    let upstream_metadata = path.join("debian/upstream/metadata");
    if upstream_metadata.exists() {
        if let Ok(mut cands) = candidates_from_upstream_metadata(
            &upstream_metadata,
            good_upstream_versions,
            net_access,
        ) {
            candidates.append(&mut cands);
        }
    }

    // Check for Cabal files (Haskell packages)
    if let Ok(entries) = std::fs::read_dir(path) {
        if let Some(cabal_file) = entries.flatten().find_map(|entry| {
            let filename = entry.file_name();
            filename
                .to_str()
                .filter(|s| s.ends_with(".cabal"))
                .map(|s| s.trim_end_matches(".cabal").to_string())
        }) {
            if let Ok(mut cands) =
                candidates_from_hackage(&cabal_file, good_upstream_versions, net_access)
            {
                candidates.append(&mut cands);
            }
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
    _good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Option<WatchCandidate>, Box<dyn std::error::Error>> {
    // Use Python to extract project name and version from setup.py
    // We monkey-patch setup() to capture the arguments
    let script = r#"
import sys
import os
import setuptools

setup_args = {}

def capture_setup(**kwargs):
    setup_args.update(kwargs)

# Patch setuptools.setup and the distutils compatibility layer
setuptools.setup = capture_setup
setuptools._distutils.core.setup = capture_setup

# Execute the setup.py file
sys.path.insert(0, os.path.dirname(sys.argv[1]))
with open(sys.argv[1], 'r') as f:
    code = compile(f.read(), sys.argv[1], 'exec')
    exec(code)

if 'name' in setup_args:
    print(setup_args['name'])
    if 'version' in setup_args:
        print(setup_args['version'])
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
    let lines: Vec<&str> = stdout.trim().lines().collect();

    if lines.is_empty() {
        return Ok(None);
    }

    let project = lines[0].trim();
    if project.is_empty() {
        return Ok(None);
    }

    let version = if lines.len() > 1 {
        Some(lines[1].trim())
    } else {
        None
    };

    let mut certainty = Certainty::Likely;
    let mut options = Vec::new();

    // If net access is allowed, verify the package exists on PyPI
    if net_access {
        if let Some(version_str) = version {
            let json_url = format!("https://pypi.python.org/pypi/{}/json", project);
            if let Ok(pypi_data) = load_json(&json_url) {
                let releases = pypi_data["releases"].as_object();
                let release_files = releases
                    .and_then(|r| r.get(version_str))
                    .and_then(|v| v.as_array());

                if let Some(files) = release_files {
                    certainty = Certainty::Certain;

                    // Check if any sdist has a signature
                    let filename_regex = regex::Regex::new(&format!(
                        r"{}-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))",
                        regex::escape(project)
                    ))?;

                    let has_signature = files.iter().any(|file| {
                        file["packagetype"].as_str() == Some("sdist")
                            && file["filename"]
                                .as_str()
                                .is_some_and(|f| filename_regex.is_match(f))
                            && file["has_sig"].as_bool() == Some(true)
                    });

                    if has_signature {
                        options.push(debian_watch::WatchOption::Pgpsigurlmangle(
                            "s/$/.asc/".to_string(),
                        ));
                    }
                }
            }
        }
    }

    // Create watch entry for PyPI
    let source = format!("https://pypi.debian.net/{}/", project);
    let matching_pattern = format!(
        r"{}-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))",
        regex::escape(project)
    );

    Ok(Some(WatchCandidate {
        source,
        matching_pattern,
        options,
        site: "pypi".to_string(),
        certainty: Some(certainty),
        preference: 1,
    }))
}

/// Generate watch entry for CRAN packages
fn guess_cran_watch_entry(name: &str) -> Result<WatchCandidate, Box<dyn std::error::Error>> {
    let source = "https://cran.r-project.org/src/contrib/".to_string();
    let matching_pattern = format!("{}_([-.\\d]*)\\.tar\\.gz", name);

    Ok(WatchCandidate {
        source,
        matching_pattern,
        options: vec![],
        site: "cran".to_string(),
        certainty: Some(Certainty::Likely),
        preference: 0,
    })
}

/// Generate watch entry for GitHub repos
fn guess_github_watch_entry(
    parsed_url: &Url,
    good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    if !net_access {
        return Ok(vec![]);
    }

    // Open the branch using breezyshim
    let branch = breezyshim::branch::open_as_generic(parsed_url)?;
    let tags = branch.tags()?.get_tag_dict()?;

    let possible_patterns = vec![r"v(\d\S+)", r"(\d\S+)", r".*/[vV]?(\d[^\s+]+)\.tar\.gz"];

    let mut version_pattern = None;
    let mut tag_names: Vec<String> = tags.keys().cloned().collect();
    tag_names.sort();
    tag_names.reverse();

    for name in &tag_names {
        for pattern in &possible_patterns {
            let re = regex::Regex::new(pattern)?;
            if let Some(m) = re.captures(name) {
                if let Some(version) = m.get(1) {
                    if good_upstream_versions.contains(&version.as_str().to_string()) {
                        version_pattern = Some(pattern.to_string());
                        break;
                    }
                }
            }
        }
        if version_pattern.is_some() {
            break;
        }
    }

    let version_pattern = match version_pattern {
        Some(p) => p,
        None => return Ok(vec![]),
    };

    let path_parts: Vec<&str> = parsed_url.path().trim_matches('/').split('/').collect();
    if path_parts.len() < 2 {
        return Ok(vec![]);
    }

    let username = path_parts[0];
    let mut project = path_parts[1].to_string();
    if project.ends_with(".git") {
        project = project[..project.len() - 4].to_string();
    }

    let source = format!("https://github.com/{}/{}/tags", username, project);
    let matching_pattern = format!(r".*/{}\\.tar\\.gz", version_pattern);

    // Create watch entry with filenamemangle
    let filemangle = format!("s/{}/{}-$1\\.tar\\.gz/", matching_pattern, project);
    let options = vec![debian_watch::WatchOption::Filenamemangle(filemangle)];

    Ok(vec![WatchCandidate {
        source,
        matching_pattern,
        options,
        site: "github".to_string(),
        certainty: Some(Certainty::Certain),
        preference: 0,
    }])
}

/// Generate watch entry for Launchpad projects
fn guess_launchpad_watch_entry(
    parsed_url: &Url,
    _good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    if !net_access {
        return Ok(vec![]);
    }

    let path_parts: Vec<&str> = parsed_url.path().trim_matches('/').split('/').collect();
    if path_parts.is_empty() {
        return Ok(vec![]);
    }

    let project = path_parts[0];
    let url = format!("https://api.launchpad.net/devel/{}/releases", project);

    let mut entries = Vec::new();
    let mut next_url = Some(url);

    while let Some(current_url) = next_url {
        let response = load_json(&current_url)?;
        if let Some(arr) = response["entries"].as_array() {
            entries.extend(arr.iter().cloned());
        }
        next_url = response["next_collection_link"]
            .as_str()
            .map(|s| s.to_string());
    }

    if entries.is_empty() {
        return Ok(vec![]);
    }

    let last_entry = &entries[entries.len() - 1];
    let files_url = last_entry["files_collection_link"]
        .as_str()
        .ok_or("Missing files_collection_link")?;

    let files = load_json(files_url)?;
    let file_entries = files["entries"]
        .as_array()
        .ok_or("Missing entries in files")?;

    if file_entries.is_empty() {
        return Ok(vec![]);
    }

    let file_link = file_entries[0]["file_link"]
        .as_str()
        .ok_or("Missing file_link")?;
    let version = last_entry["version"].as_str().ok_or("Missing version")?;

    let file_parts: Vec<&str> = file_link.split('/').collect();
    if file_parts.len() < 2 {
        return Ok(vec![]);
    }
    let filepattern = file_parts[file_parts.len() - 2].replace(version, "(.*)");

    let source = format!("https://launchpad.net/{}/+download", project);
    let matching_pattern = format!("https://launchpad.net/{}/.*/{}", project, filepattern);

    Ok(vec![WatchCandidate {
        source,
        matching_pattern,
        options: vec![],
        site: "launchpad".to_string(),
        certainty: Some(Certainty::Certain),
        preference: 0,
    }])
}

/// Extract watch candidates from Hackage
fn candidates_from_hackage(
    package: &str,
    good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    if !net_access {
        return Ok(vec![]);
    }

    let url = format!("https://hackage.haskell.org/package/{}/preferred", package);
    let versions = match load_json(&url) {
        Ok(v) => v,
        Err(_) => return Ok(vec![]),
    };

    let normal_versions = versions["normal-version"]
        .as_array()
        .ok_or("Missing normal-version")?;

    let mut found = false;
    for version in normal_versions {
        if let Some(v) = version.as_str() {
            if good_upstream_versions.contains(&v.to_string()) {
                found = true;
                break;
            }
        }
    }

    if !found {
        return Ok(vec![]);
    }

    let source = format!("https://hackage.haskell.org/package/{}", package);
    let matching_pattern = format!(r".*/{}-(.*)\.tar\.gz", regex::escape(package));

    Ok(vec![WatchCandidate {
        source,
        matching_pattern,
        options: vec![],
        site: "hackage".to_string(),
        certainty: Some(Certainty::Certain),
        preference: 1,
    }])
}

/// Extract watch candidates from debian/upstream/metadata
fn candidates_from_upstream_metadata(
    path: &Path,
    good_upstream_versions: &[String],
    net_access: bool,
) -> Result<Vec<WatchCandidate>, Box<dyn std::error::Error>> {
    use serde_yaml::Value;
    use std::fs;

    let content = fs::read_to_string(path)?;
    let yaml: Value = serde_yaml::from_str(&content)?;

    let mut candidates = Vec::new();

    // Check for Repository or X-Download fields
    for field in ["Repository", "X-Download"] {
        if let Some(url_str) = yaml.get(field).and_then(|v| v.as_str()) {
            let url_parts: Vec<&str> = url_str.split_whitespace().collect();
            if url_parts.is_empty() {
                continue;
            }

            if let Ok(parsed_url) = Url::parse(url_parts[0]) {
                if parsed_url.host_str() == Some("github.com") {
                    if let Ok(mut cands) =
                        guess_github_watch_entry(&parsed_url, good_upstream_versions, net_access)
                    {
                        candidates.append(&mut cands);
                    }
                }
                if parsed_url.host_str() == Some("launchpad.net") {
                    if let Ok(mut cands) =
                        guess_launchpad_watch_entry(&parsed_url, good_upstream_versions, net_access)
                    {
                        candidates.append(&mut cands);
                    }
                }
            }
        }
    }

    // Check for CRAN packages
    if let Some(archive) = yaml.get("Archive").and_then(|v| v.as_str()) {
        if archive == "CRAN" {
            if let Some(name) = yaml.get("Name").and_then(|v| v.as_str()) {
                if let Ok(cand) = guess_cran_watch_entry(name) {
                    candidates.push(cand);
                }
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

    // Create a new v5 watch file with the entry
    let mut watch_file = debian_watch::parse::ParsedWatchFile::new(5)
        .map_err(|e| FixerError::Other(format!("Failed to create watch file: {}", e)))?;
    let mut entry = watch_file.add_entry(&winner.source, &winner.matching_pattern);

    // Apply any options to the entry
    for option in winner.options {
        entry.set_option(option);
    }

    // Write the watch file
    std::fs::write(&watch_path, watch_file.to_string())?;

    let mut result = FixerResult::builder(format!("Add debian/watch file, using {}.", winner.site));

    if let Some(certainty) = winner.certainty {
        result = result.certainty(certainty);
    }

    result = result.fixed_issues(vec![issue]);

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
        assert!(candidates[0].source.contains("pypi.debian.net/xandikos"));
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
            .source
            .contains("cran.r-project.org/src/contrib"));
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
