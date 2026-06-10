use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::tree::WorkingTree;
use breezyshim::workingtree::GenericWorkingTree;
use debian_analyzer::{
    add_changelog_entry, apply_or_revert, certainty_sufficient, get_committer, ApplyError,
    ChangelogError,
};
use debian_control::fields::MultiArch;
use debian_workspace::action::{Action, ActionPlan, Deb822Action, ParagraphSelector};
use debian_workspace::appliers::apply_actions;
use debian_workspace::workspace::Workspace;
use debversion::Version;
use lazy_regex::regex_captures;
use lazy_static::lazy_static;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_yaml::from_value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

/// Re-export so library consumers (e.g. an LSP host) don't have to
/// add a direct `debian-analyzer` dep just to spell the certainty
/// parameter to [`detect_multiarch_hints`].
pub use debian_analyzer::Certainty;

pub const MULTIARCH_HINTS_URL: &str = "https://dedup.debian.net/static/multiarch-hints.yaml.xz";
const USER_AGENT: &str = concat!("apply-multiarch-hints/", env!("CARGO_PKG_VERSION"));

const DEFAULT_VALUE_MULTIARCH_HINT: i32 = 100;

#[derive(Debug, Clone, Copy, std::hash::Hash, PartialEq, Eq)]
pub enum HintKind {
    MaForeign,
    FileConflict,
    MaForeignLibrary,
    DepAny,
    MaSame,
    ArchAll,
    MaWorkaround,
}

impl std::str::FromStr for HintKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ma-foreign" => Ok(HintKind::MaForeign),
            "file-conflict" => Ok(HintKind::FileConflict),
            "ma-foreign-library" => Ok(HintKind::MaForeignLibrary),
            "dep-any" => Ok(HintKind::DepAny),
            "ma-same" => Ok(HintKind::MaSame),
            "arch-all" => Ok(HintKind::ArchAll),
            "ma-workaround" => Ok(HintKind::MaWorkaround),
            _ => Err(format!("Invalid hint kind: {:?}", s)),
        }
    }
}

fn hint_value(hint: HintKind) -> i32 {
    match hint {
        HintKind::MaForeign => 20,
        HintKind::FileConflict => 50,
        HintKind::MaForeignLibrary => 20,
        HintKind::DepAny => 20,
        HintKind::MaSame => 20,
        HintKind::ArchAll => 20,
        HintKind::MaWorkaround => 20,
    }
}

pub fn calculate_value(hints: &[HintKind]) -> i32 {
    hints.iter().map(|hint| hint_value(*hint)).sum::<i32>() + DEFAULT_VALUE_MULTIARCH_HINT
}

fn format_system_time(system_time: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Utc> = system_time.into();
    datetime.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Ord, PartialOrd, Clone, Copy)]
pub enum Severity {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "high")]
    High,
}

fn deserialize_severity<'de, D>(deserializer: D) -> Result<Severity, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "low" => Ok(Severity::Low),
        "normal" => Ok(Severity::Normal),
        "high" => Ok(Severity::High),
        _ => Err(serde::de::Error::custom(format!(
            "Invalid severity: {:?}",
            s
        ))),
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Hint {
    pub binary: String,
    pub description: String,
    #[serde(default)]
    pub source: Option<String>,
    pub link: String,
    #[serde(deserialize_with = "deserialize_severity")]
    pub severity: Severity,
    pub version: Option<Version>,
}

impl Hint {
    pub fn kind(&self) -> &str {
        self.link.split('#').next_back().unwrap()
    }
}

pub fn multiarch_hints_by_source(hints: &[Hint]) -> HashMap<&str, Vec<&Hint>> {
    let mut map = HashMap::new();
    for hint in hints {
        if let Some(source) = hint.source.as_deref() {
            map.entry(source).or_insert_with(Vec::new).push(hint);
        }
    }
    map
}

pub fn multiarch_hints_by_binary(hints: &[Hint]) -> HashMap<&str, Vec<&Hint>> {
    let mut map = HashMap::new();
    for hint in hints {
        map.entry(hint.binary.as_str())
            .or_insert_with(Vec::new)
            .push(hint);
    }
    map
}

pub fn parse_multiarch_hints(f: &[u8]) -> Result<Vec<Hint>, serde_yaml::Error> {
    let data = serde_yaml::from_slice::<serde_yaml::Value>(f)?;
    if let Some(format) = data["format"].as_str() {
        if format != "multiarch-hints-1.0" {
            return Err(serde::de::Error::custom(format!(
                "Invalid format: {:?}",
                format
            )));
        }
    } else {
        return Err(serde::de::Error::custom("Missing format"));
    }
    from_value(data["hints"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_some_entries() {
        let hints = parse_multiarch_hints(
            r#"format: multiarch-hints-1.0
hints:
- binary: coinor-libcoinmp-dev
  description: coinor-libcoinmp-dev conflicts on ...
  link: https://wiki.debian.org/MultiArch/Hints#file-conflict
  severity: high
  source: coinmp
  version: 1.8.3-2+b11
"#
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            hints,
            vec![Hint {
                binary: "coinor-libcoinmp-dev".to_string(),
                description: "coinor-libcoinmp-dev conflicts on ...".to_string(),
                link: "https://wiki.debian.org/MultiArch/Hints#file-conflict".to_string(),
                severity: Severity::High,
                version: Some("1.8.3-2+b11".parse().unwrap()),
                source: Some("coinmp".to_string()),
            }]
        );
    }

    #[test]
    fn test_missing_source() {
        let hints = parse_multiarch_hints(
            r#"format: multiarch-hints-1.0
hints:
- binary: somepkg
  description: some description
  link: https://wiki.debian.org/MultiArch/Hints#file-conflict
  severity: high
"#
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            hints,
            vec![Hint {
                binary: "somepkg".to_string(),
                description: "some description".to_string(),
                link: "https://wiki.debian.org/MultiArch/Hints#file-conflict".to_string(),
                severity: Severity::High,
                version: None,
                source: None,
            }]
        );
    }

    #[test]
    fn test_invalid_header() {
        let hints = parse_multiarch_hints(
            r#"\
format: blah
"#
            .as_bytes(),
        );
        assert!(hints.is_err());
    }

    fn make_hint(binary: &str, kind: &str, description: &str) -> Hint {
        Hint {
            binary: binary.to_string(),
            description: description.to_string(),
            link: format!("https://wiki.debian.org/MultiArch/Hints#{}", kind),
            severity: Severity::Normal,
            version: None,
            source: Some("src".to_string()),
        }
    }

    fn setup_ws(
        control: &str,
    ) -> (
        tempfile::TempDir,
        debian_workspace::fs_workspace::FsWorkspace,
    ) {
        let tmp = tempfile::TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        std::fs::create_dir_all(&debian).unwrap();
        std::fs::write(debian.join("control"), control).unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("src".into()),
            Some("1.0".parse().unwrap()),
        );
        (tmp, ws)
    }

    fn detect_one(
        ws: &debian_workspace::fs_workspace::FsWorkspace,
        hints_list: &[Hint],
    ) -> Vec<(Change, ActionPlan)> {
        let by_binary = multiarch_hints_by_binary(hints_list);
        detect_multiarch_hints(ws, &by_binary, Certainty::Possible).unwrap()
    }

    fn apply_and_read(
        ws: &debian_workspace::fs_workspace::FsWorkspace,
        plans: &[ActionPlan],
    ) -> String {
        let actions: Vec<_> = plans
            .iter()
            .flat_map(|p| p.actions.iter().cloned())
            .collect();
        debian_workspace::appliers::apply_actions(ws.base_path(), &actions).unwrap();
        std::fs::read_to_string(ws.base_path().join("debian/control")).unwrap()
    }

    #[test]
    fn detect_ma_foreign() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\n");
        let hints = vec![make_hint("foo", "ma-foreign", "foo could be MA: foreign")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.binary, "foo");
        assert_eq!(results[0].0.description, "Add Multi-Arch: foreign.");

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(control.contains("Multi-Arch: foreign"), "got: {}", control);
    }

    #[test]
    fn detect_ma_foreign_noop_when_already_set() {
        let (_tmp, ws) =
            setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: foreign\n");
        let hints = vec![make_hint("foo", "ma-foreign", "foo could be MA: foreign")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn detect_ma_foreign_library() {
        let (_tmp, ws) =
            setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: foreign\n");
        let hints = vec![make_hint(
            "foo",
            "ma-foreign-library",
            "foo should not be MA: foreign",
        )];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.description, "Drop Multi-Arch: foreign.");

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(!control.contains("Multi-Arch"), "got: {}", control);
    }

    #[test]
    fn detect_file_conflict() {
        let (_tmp, ws) =
            setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\nMulti-Arch: same\n");
        let hints = vec![make_hint("foo", "file-conflict", "foo conflicts")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.description, "Drop Multi-Arch: same.");

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(!control.contains("Multi-Arch"), "got: {}", control);
    }

    #[test]
    fn detect_ma_same() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\n");
        let hints = vec![make_hint("foo", "ma-same", "foo should be MA: same")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.description, "Add Multi-Arch: same.");

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(control.contains("Multi-Arch: same"), "got: {}", control);
    }

    #[test]
    fn detect_arch_all() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\n");
        let hints = vec![make_hint("foo", "arch-all", "foo should be arch:all")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.description, "Make package Architecture: all.");
        assert_eq!(results[0].0.certainty, Certainty::Possible);

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(control.contains("Architecture: all"), "got: {}", control);
    }

    #[test]
    fn detect_arch_all_noop_when_already_all() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: all\n");
        let hints = vec![make_hint("foo", "arch-all", "foo should be arch:all")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn detect_dep_any() {
        let (_tmp, ws) =
            setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\nDepends: libbar (>= 1.0)\n");
        let hints = vec![make_hint(
            "foo",
            "dep-any",
            "foo could have its dependency on libbar annotated with :any",
        )];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].0.description,
            "Add :any qualifier for libbar dependency."
        );

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(control.contains("libbar:any"), "got: {}", control);
    }

    #[test]
    fn detect_dep_any_noop_when_already_annotated() {
        let (_tmp, ws) = setup_ws(
            "Source: src\n\nPackage: foo\nArchitecture: any\nDepends: libbar:any (>= 1.0)\n",
        );
        let hints = vec![make_hint(
            "foo",
            "dep-any",
            "foo could have its dependency on libbar annotated with :any",
        )];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn detect_ma_workaround() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: all\n");
        let hints = vec![make_hint(
            "foo",
            "ma-workaround",
            "foo should be Architecture: any + Multi-Arch: same",
        )];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].0.description,
            "Add Multi-Arch: same and set Architecture: any."
        );

        let control = apply_and_read(
            &ws,
            &results.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        );
        assert!(control.contains("Multi-Arch: same"), "got: {}", control);
        assert!(control.contains("Architecture: any"), "got: {}", control);
    }

    #[test]
    fn detect_skips_unknown_binary() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\n");
        let hints = vec![make_hint("bar", "ma-foreign", "bar could be MA: foreign")];
        let results = detect_one(&ws, &hints);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn detect_skips_below_minimum_certainty() {
        let (_tmp, ws) = setup_ws("Source: src\n\nPackage: foo\nArchitecture: any\n");
        let hints = vec![make_hint("foo", "arch-all", "foo should be arch:all")];
        let by_binary = multiarch_hints_by_binary(&hints);
        // arch-all is Possible; requesting Certain should skip it.
        let results = detect_multiarch_hints(&ws, &by_binary, Certainty::Certain).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn detect_no_control_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            tmp.path(),
            Some("src".into()),
            Some("1.0".parse().unwrap()),
        );
        let hints = vec![make_hint("foo", "ma-foreign", "foo could be MA: foreign")];
        let by_binary = multiarch_hints_by_binary(&hints);
        let results = detect_multiarch_hints(&ws, &by_binary, Certainty::Possible).unwrap();
        assert_eq!(results.len(), 0);
    }
}

/// Locate the directory we cache the downloaded multiarch-hints file in.
///
/// Honours `$XDG_CACHE_HOME` and falls back to `$HOME/.cache`. Returns
/// `None` when neither is set — callers should treat that as "skip the
/// cache" rather than as an error. The returned path is *not* created;
/// see [`cache_file_path`] for the canonical filename.
pub fn cache_dir() -> Option<std::path::PathBuf> {
    let cache_home = if let Ok(xdg_cache_home) = std::env::var("XDG_CACHE_HOME") {
        Path::new(&xdg_cache_home).to_path_buf()
    } else if let Ok(home) = std::env::var("HOME") {
        Path::new(&home).join(".cache")
    } else {
        return None;
    };
    Some(cache_home.join("lintian-brush"))
}

/// Path to the cached multiarch-hints file, or `None` when no cache
/// directory is available. The directory is *not* created here — the
/// sync/async cache wrappers call their respective `create_dir_all`
/// before writing.
pub fn cache_file_path() -> Option<std::path::PathBuf> {
    cache_dir().map(|d| d.join("multiarch-hints.yml"))
}

pub fn cache_download_multiarch_hints(
    url: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let Some(local_hints_path) = cache_file_path() else {
        log::warn!("Unable to find cache directory, not caching");
        return download_multiarch_hints(url, None)?
            .ok_or_else(|| "Expected download data but got None".into());
    };
    if let Some(parent) = local_hints_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let last_modified = match fs::metadata(&local_hints_path) {
        Ok(metadata) => Some(metadata.modified()?),
        Err(_) => None,
    };

    match download_multiarch_hints(url, last_modified) {
        Ok(None) => {
            let mut buffer = Vec::new();
            std::fs::File::open(&local_hints_path)?.read_to_end(&mut buffer)?;
            Ok(buffer)
        }
        Ok(Some(buffer)) => {
            fs::File::create(&local_hints_path)?.write_all(&buffer)?;
            Ok(buffer)
        }
        Err(e) => Err(e),
    }
}

pub fn download_multiarch_hints(
    url: Option<&str>,
    since: Option<SystemTime>,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    let url = url.unwrap_or(MULTIARCH_HINTS_URL);
    let client = Client::builder().user_agent(USER_AGENT).build()?;
    let mut request = client.get(url).header("Accept-Encoding", "identity");
    if let Some(since) = since {
        request = request.header("If-Modified-Since", format_system_time(since));
    }
    let response = request.send()?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        Ok(None)
    } else if response.status() != reqwest::StatusCode::OK {
        Err(format!(
            "Unable to download multiarch hints: {:?}",
            response.status()
        )
        .into())
    } else if url.ends_with(".xz") {
        // It would be nicer if there was a content-type, but there isn't :-(
        let mut reader = xz2::read::XzDecoder::new(response);
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;
        Ok(Some(buffer))
    } else {
        Ok(Some(response.bytes()?.to_vec()))
    }
}

/// Async sibling of [`download_multiarch_hints`].
///
/// Performs the HTTP request on reqwest's async client (so the caller's
/// tokio runtime stays unblocked) and decompresses the `.xz` payload on
/// the blocking pool, where the CPU-bound work belongs.
#[cfg(feature = "async")]
pub async fn download_multiarch_hints_async(
    url: Option<&str>,
    since: Option<SystemTime>,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    let url = url.unwrap_or(MULTIARCH_HINTS_URL).to_string();
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let mut request = client.get(&url).header("Accept-Encoding", "identity");
    if let Some(since) = since {
        request = request.header("If-Modified-Since", format_system_time(since));
    }
    let response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    } else if response.status() != reqwest::StatusCode::OK {
        return Err(format!(
            "Unable to download multiarch hints: {:?}",
            response.status()
        )
        .into());
    }
    let bytes = response.bytes().await?.to_vec();
    if url.ends_with(".xz") {
        // xz decompression is CPU-bound; keep it off the async worker.
        let decoded = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut reader = xz2::read::XzDecoder::new(&bytes[..]);
            let mut out = Vec::new();
            reader.read_to_end(&mut out)?;
            Ok::<Vec<u8>, std::io::Error>(out)
        })
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })??;
        Ok(Some(decoded))
    } else {
        Ok(Some(bytes))
    }
}

/// Async sibling of [`cache_download_multiarch_hints`].
///
/// Conditional GET against the cached copy on disk; refreshes it from the
/// network when stale and returns the live bytes either way.
#[cfg(feature = "async")]
pub async fn cache_download_multiarch_hints_async(
    url: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(local_hints_path) = cache_file_path() else {
        log::warn!("Unable to find cache directory, not caching");
        return download_multiarch_hints_async(url, None)
            .await?
            .ok_or_else(|| "Expected download data but got None".into());
    };
    if let Some(parent) = local_hints_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let last_modified = match tokio::fs::metadata(&local_hints_path).await {
        Ok(metadata) => Some(metadata.modified()?),
        Err(_) => None,
    };

    match download_multiarch_hints_async(url, last_modified).await {
        Ok(None) => Ok(tokio::fs::read(&local_hints_path).await?),
        Ok(Some(buffer)) => {
            tokio::fs::write(&local_hints_path, &buffer).await?;
            Ok(buffer)
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone)]
pub struct Change {
    pub binary: String,
    pub hint: Hint,
    pub description: String,
    pub certainty: Certainty,
}

pub struct OverallResult {
    pub changes: Vec<Change>,
}

impl OverallResult {
    pub fn value(&self) -> i32 {
        let kinds = self
            .changes
            .iter()
            .map(|x| x.hint.kind().parse().unwrap())
            .collect::<Vec<_>>();
        calculate_value(&kinds)
    }
}

fn control_file() -> std::path::PathBuf {
    std::path::PathBuf::from("debian/control")
}

fn detect_hint_ma_foreign(
    binary: &debian_control::lossless::control::Binary,
    _hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    if binary.multi_arch() == Some(MultiArch::Foreign) {
        return None;
    }
    let pkg = binary.name()?;
    Some((
        "Add Multi-Arch: foreign.".to_string(),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Multi-Arch".to_string(),
            value: "foreign".to_string(),
        })],
    ))
}

fn detect_hint_ma_foreign_lib(
    binary: &debian_control::lossless::control::Binary,
    _hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    if binary.multi_arch() != Some(MultiArch::Foreign) {
        return None;
    }
    let pkg = binary.name()?;
    Some((
        "Drop Multi-Arch: foreign.".to_string(),
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Multi-Arch".to_string(),
        })],
    ))
}

fn detect_hint_file_conflict(
    binary: &debian_control::lossless::control::Binary,
    _hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    if binary.multi_arch() != Some(MultiArch::Same) {
        return None;
    }
    let pkg = binary.name()?;
    Some((
        "Drop Multi-Arch: same.".to_string(),
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Multi-Arch".to_string(),
        })],
    ))
}

fn detect_hint_ma_same(
    binary: &debian_control::lossless::control::Binary,
    _hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    if binary.multi_arch() == Some(MultiArch::Same) {
        return None;
    }
    let pkg = binary.name()?;
    Some((
        "Add Multi-Arch: same.".to_string(),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Multi-Arch".to_string(),
            value: "same".to_string(),
        })],
    ))
}

fn detect_hint_arch_all(
    binary: &debian_control::lossless::control::Binary,
    _hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    if binary.architecture().as_deref() == Some("all") {
        return None;
    }
    let pkg = binary.name()?;
    Some((
        "Make package Architecture: all.".to_string(),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Architecture".to_string(),
            value: "all".to_string(),
        })],
    ))
}

fn detect_hint_dep_any(
    binary: &debian_control::lossless::control::Binary,
    hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    let Some((_whole, binary_package, dep)) = regex_captures!(
        r"(.*) could have its dependency on (.*) annotated with :any",
        hint.description.as_str()
    ) else {
        log::warn!("Unable to parse dep-any hint: {:?}", hint.description);
        return None;
    };
    assert_eq!(binary_package, binary.name().unwrap());

    let depends = binary.depends()?;
    let entry_text = depends.entries().find_map(|entry| {
        entry.relations().find_map(|r| {
            if r.try_name().as_deref() == Some(dep) && r.archqual().as_deref() != Some("any") {
                // Re-serialise the entry with :any added. We rebuild it from
                // the entry's display form to preserve version constraints.
                Some(entry.to_string().replacen(dep, &format!("{}:any", dep), 1))
            } else {
                None
            }
        })
    })?;

    let pkg = binary.name()?;
    Some((
        format!("Add :any qualifier for {} dependency.", dep),
        vec![Action::Deb822(Deb822Action::ReplaceRelation {
            file: control_file(),
            paragraph: ParagraphSelector::Binary { package: pkg },
            field: "Depends".to_string(),
            from_package: dep.to_string(),
            to_entry: entry_text,
        })],
    ))
}

fn detect_hint_ma_workaround(
    binary: &debian_control::lossless::control::Binary,
    hint: &Hint,
) -> Option<(String, Vec<Action>)> {
    let Some((_whole, binary_package)) = regex_captures!(
        r"(.*) should be Architecture: any \+ Multi-Arch: same",
        hint.description.as_str()
    ) else {
        log::warn!("Unable to parse ma-workaround hint: {:?}", hint.description);
        return None;
    };
    assert_eq!(binary_package, binary.name().unwrap());
    let pkg = binary.name()?;
    Some((
        "Add Multi-Arch: same and set Architecture: any.".to_string(),
        vec![
            Action::Deb822(Deb822Action::SetField {
                file: control_file(),
                paragraph: ParagraphSelector::Binary {
                    package: pkg.clone(),
                },
                field: "Multi-Arch".to_string(),
                value: "same".to_string(),
            }),
            Action::Deb822(Deb822Action::SetField {
                file: control_file(),
                paragraph: ParagraphSelector::Binary { package: pkg },
                field: "Architecture".to_string(),
                value: "any".to_string(),
            }),
        ],
    ))
}

type DetectorFn =
    fn(&debian_control::lossless::control::Binary, &Hint) -> Option<(String, Vec<Action>)>;

struct Detector {
    kind: &'static str,
    certainty: Certainty,
    cb: DetectorFn,
}

lazy_static! {
    static ref DETECTORS: Vec<Detector> = vec![
        Detector {
            kind: "ma-foreign",
            certainty: Certainty::Certain,
            cb: detect_hint_ma_foreign,
        },
        Detector {
            kind: "file-conflict",
            certainty: Certainty::Certain,
            cb: detect_hint_file_conflict,
        },
        Detector {
            kind: "ma-foreign-library",
            certainty: Certainty::Certain,
            cb: detect_hint_ma_foreign_lib,
        },
        Detector {
            kind: "dep-any",
            certainty: Certainty::Certain,
            cb: detect_hint_dep_any,
        },
        Detector {
            kind: "ma-same",
            certainty: Certainty::Certain,
            cb: detect_hint_ma_same,
        },
        Detector {
            kind: "arch-all",
            certainty: Certainty::Possible,
            cb: detect_hint_arch_all,
        },
        Detector {
            kind: "ma-workaround",
            certainty: Certainty::Certain,
            cb: detect_hint_ma_workaround,
        },
    ];
}

fn find_detector(kind: &str) -> Option<&'static Detector> {
    DETECTORS.iter().find(|x| x.kind == kind)
}

/// Detect which multiarch hints apply to the given workspace.
///
/// Returns one `(Change, ActionPlan)` per applicable hint. The `Change`
/// describes what would change (for commit messages / logging); the
/// `ActionPlan` carries the file edits to apply via `apply_actions`.
pub fn detect_multiarch_hints(
    ws: &dyn Workspace,
    hints: &HashMap<&str, Vec<&Hint>>,
    minimum_certainty: Certainty,
) -> Result<Vec<(Change, ActionPlan)>, debian_workspace::Error> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut results = Vec::new();
    for binary in control.binaries() {
        let Some(package) = binary.name() else {
            continue;
        };
        let Some(package_hints) = hints.get(package.as_str()) else {
            continue;
        };
        for hint in package_hints {
            let kind = hint.kind();
            let detector = match find_detector(kind) {
                Some(d) => d,
                None => {
                    log::warn!("Unknown hint kind: {}", kind);
                    continue;
                }
            };
            if !certainty_sufficient(detector.certainty, Some(minimum_certainty)) {
                continue;
            }
            if let Some((description, actions)) = (detector.cb)(&binary, hint) {
                results.push((
                    Change {
                        binary: package.clone(),
                        hint: (*hint).clone(),
                        description: description.clone(),
                        certainty: detector.certainty,
                    },
                    ActionPlan {
                        label: description,
                        opinionated: false,
                        certainty: None,
                        actions,
                    },
                ));
            }
        }
    }
    Ok(results)
}

fn changes_by_description(changes: &[Change]) -> HashMap<String, Vec<String>> {
    let mut by_description: HashMap<String, Vec<String>> = HashMap::new();
    for change in changes {
        by_description
            .entry(change.description.clone())
            .or_default()
            .push(change.binary.clone());
    }
    by_description
}

#[derive(Debug)]
pub enum OverallError {
    BrzError(Error),
    NotDebianPackage(std::path::PathBuf),
    Other(String),
    NoWhoami,
    NoChanges,
    GeneratedFile(std::path::PathBuf),
    FormattingUnpreservable(std::path::PathBuf),
}

impl From<debian_analyzer::editor::EditorError> for OverallError {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::GeneratedFile(p, _) => {
                OverallError::GeneratedFile(p)
            }
            debian_analyzer::editor::EditorError::FormattingUnpreservable(p, _) => {
                OverallError::FormattingUnpreservable(p)
            }
            debian_analyzer::editor::EditorError::BrzError(e) => OverallError::BrzError(e),
            debian_analyzer::editor::EditorError::IoError(e) => OverallError::Other(e.to_string()),
            debian_analyzer::editor::EditorError::TemplateError(p, _e) => {
                OverallError::GeneratedFile(p)
            }
        }
    }
}

impl std::fmt::Display for OverallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OverallError::NotDebianPackage(p) => {
                write!(f, "{} is not a Debian package.", p.display())
            }
            OverallError::GeneratedFile(p) => {
                write!(f, "Generated file: {}", p.display())
            }
            OverallError::FormattingUnpreservable(p) => {
                write!(f, "Formatting unpreservable: {}", p.display())
            }
            OverallError::BrzError(e) => write!(f, "{}", e),
            OverallError::NoWhoami => write!(f, "No committer configured."),
            OverallError::NoChanges => write!(f, "No changes to apply."),
            OverallError::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for OverallError {}

impl From<Error> for OverallError {
    fn from(e: Error) -> Self {
        match e {
            Error::PointlessCommit => OverallError::NoChanges,
            Error::NoWhoami => OverallError::NoWhoami,
            Error::Other(e) => OverallError::Other(e.to_string()),
            e => OverallError::BrzError(e),
        }
    }
}

impl From<ChangelogError> for OverallError {
    fn from(e: ChangelogError) -> Self {
        match e {
            ChangelogError::NotDebianPackage(p) => OverallError::NotDebianPackage(p),
            ChangelogError::Python(e) => OverallError::Other(e.to_string()),
        }
    }
}

/// Configuration options for applying multiarch hints
#[derive(Debug, Clone)]
pub struct ApplyMultiarchHintsConfig {
    pub minimum_certainty: Option<Certainty>,
    pub committer: Option<String>,
    pub update_changelog: bool,
    pub allow_reformatting: Option<bool>,
}

impl Default for ApplyMultiarchHintsConfig {
    fn default() -> Self {
        Self {
            minimum_certainty: None,
            committer: None,
            update_changelog: true,
            allow_reformatting: None,
        }
    }
}

#[allow(clippy::result_large_err)]
pub fn apply_multiarch_hints(
    local_tree: &GenericWorkingTree,
    subpath: &std::path::Path,
    hints: &HashMap<&str, Vec<&Hint>>,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    config: &ApplyMultiarchHintsConfig,
) -> Result<OverallResult, OverallError> {
    let minimum_certainty = config.minimum_certainty.unwrap_or(Certainty::Certain);
    let basis_tree = local_tree.basis_tree().map_err(OverallError::BrzError)?;
    let (changes, _tree_changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        &basis_tree,
        dirty_tracker,
        |path| -> Result<Vec<Change>, OverallError> {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(path, None, None);
            let detected = detect_multiarch_hints(&ws, hints, minimum_certainty)
                .map_err(|e| OverallError::Other(e.to_string()))?;

            if detected.is_empty() {
                return Ok(Vec::new());
            }

            let all_actions: Vec<_> = detected
                .iter()
                .flat_map(|(_, plan)| plan.actions.iter().cloned())
                .collect();
            apply_actions(path, &all_actions).map_err(|e| OverallError::Other(e.to_string()))?;

            Ok(detected.into_iter().map(|(change, _)| change).collect())
        },
    ) {
        Ok(r) => r,
        Err(ApplyError::NoChanges(_)) => return Err(OverallError::NoChanges),
        Err(ApplyError::BrzError(e)) => return Err(OverallError::BrzError(e)),
        Err(ApplyError::CallbackError(_)) => panic!("Unexpected callback error"),
    };

    let by_description = changes_by_description(changes.as_slice());
    let mut overall_description = vec!["Apply multi-arch hints.\n".to_string()];
    for (description, mut binaries) in by_description {
        binaries.sort();
        overall_description.push(format!(" + {}: {}\n", binaries.join(", "), description));
    }

    let changelog_path = subpath.join("debian/changelog");

    if config.update_changelog {
        add_changelog_entry(
            local_tree,
            changelog_path.as_path(),
            overall_description
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        if let Some(specific_files) = specific_files.as_mut() {
            specific_files.push(changelog_path);
        }
    }

    overall_description.push("\n".to_string());
    overall_description.push("Changes-By: apply-multiarch-hints\n".to_string());

    let committer = config
        .committer
        .clone()
        .unwrap_or_else(|| get_committer(local_tree));

    let specific_files_ref = specific_files
        .as_ref()
        .map(|x| x.iter().map(|x| x.as_path()).collect::<Vec<_>>());

    let mut builder = local_tree
        .build_commit()
        .message(overall_description.concat().as_str())
        .allow_pointless(false)
        .committer(&committer);

    if let Some(specific_files) = specific_files_ref.as_deref() {
        builder = builder.specific_files(specific_files);
    }

    builder.commit()?;

    Ok(OverallResult { changes })
}
