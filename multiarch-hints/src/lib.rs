use breezyshim::dirty_tracker::DirtyTreeTracker;
use breezyshim::error::Error;
use breezyshim::tree::WorkingTree;
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::{
    add_changelog_entry, apply_or_revert, certainty_sufficient, get_committer, ApplyError,
    Certainty, ChangelogError,
};
use debian_control::control::MultiArch;
use debian_control::control::{Binary, Source};
use debian_control::relations::Relations;
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

pub const MULTIARCH_HINTS_URL: &str = "https://dedup.debian.net/static/multiarch-hints.yaml.xz";
const USER_AGENT: &str = concat!("apply-multiarch-hints/", env!("CARGO_PKG_VERSION"));

const DEFAULT_VALUE_MULTIARCH_HINT: i32 = 100;

#[derive(Debug, Clone, Copy, std::hash::Hash, PartialEq, Eq)]
enum HintKind {
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
    pub source: String,
    pub link: String,
    #[serde(deserialize_with = "deserialize_severity")]
    pub severity: Severity,
    pub version: Option<Version>,
}

impl Hint {
    pub fn kind(&self) -> &str {
        self.link.split('#').last().unwrap()
    }
}

pub fn multiarch_hints_by_source(hints: &[Hint]) -> HashMap<&str, Vec<&Hint>> {
    let mut map = HashMap::new();
    for hint in hints {
        map.entry(hint.source.as_str())
            .or_insert_with(Vec::new)
            .push(hint);
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
                source: "coinmp".to_string(),
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
}

pub fn cache_download_multiarch_hints(
    url: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let cache_home = if let Ok(xdg_cache_home) = std::env::var("XDG_CACHE_HOME") {
        Path::new(&xdg_cache_home).to_path_buf()
    } else if let Ok(home) = std::env::var("HOME") {
        Path::new(&home).join(".cache")
    } else {
        log::warn!("Unable to find cache directory, not caching");
        return download_multiarch_hints(url, None).map(|x| x.unwrap());
    };
    let cache_dir = cache_home.join("lintian-brush");
    fs::create_dir_all(&cache_dir)?;
    let local_hints_path = cache_dir.join("multiarch-hints.yml");
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

fn apply_hint_ma_foreign(binary: &mut Binary, _hint: &Hint) -> Option<String> {
    if binary.multi_arch() != Some(MultiArch::Foreign) {
        binary.set_multi_arch(Some(MultiArch::Foreign));
        Some("Add Multi-Arch: foreign.".to_string())
    } else {
        None
    }
}

fn apply_hint_ma_foreign_lib(binary: &mut Binary, _hint: &Hint) -> Option<String> {
    if binary.multi_arch() == Some(MultiArch::Foreign) {
        binary.set_multi_arch(None);
        Some("Drop Multi-Arch: foreign.".to_string())
    } else {
        None
    }
}

fn apply_hint_file_conflict(binary: &mut Binary, _hint: &Hint) -> Option<String> {
    if binary.multi_arch() == Some(MultiArch::Same) {
        binary.set_multi_arch(None);
        Some("Drop Multi-Arch: same.".to_string())
    } else {
        None
    }
}

fn apply_hint_ma_same(binary: &mut Binary, _hint: &Hint) -> Option<String> {
    if binary.multi_arch() == Some(MultiArch::Same) {
        return None;
    }
    binary.set_multi_arch(Some(MultiArch::Same));
    Some("Add Multi-Arch: same.".to_string())
}

fn apply_hint_arch_all(binary: &mut Binary, _hint: &Hint) -> Option<String> {
    if binary.architecture().as_deref() == Some("all") {
        return None;
    }
    binary.set_architecture(Some("all"));
    Some("Make package Architecture: all.".to_string())
}

fn apply_hint_dep_any(binary: &mut Binary, hint: &Hint) -> Option<String> {
    if let Some((_whole, binary_package, dep)) = regex_captures!(
        r"(.*) could have its dependency on (.*) annotated with :any",
        hint.description.as_str()
    ) {
        assert_eq!(binary_package, binary.name().unwrap());

        let mut changed = false;
        if let Some(depends) = binary.depends() {
            for entry in depends.entries() {
                for mut r in entry.relations() {
                    if r.name() == dep && r.archqual().as_deref() != Some("any") {
                        r.set_archqual("any");
                        changed = true;
                    }
                }
            }
            if changed {
                binary.set_depends(Some(&depends));
                Some(format!("Add :any qualifier for {} dependency.", dep))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        log::warn!("Unable to parse dep-any hint: {:?}", hint.description);
        None
    }
}

fn apply_hint_ma_workaround(binary: &mut Binary, hint: &Hint) -> Option<String> {
    if let Some((_whole, binary_package)) = regex_captures!(
        r"(.*) should be Architecture: any \+ Multi-Arch: same",
        hint.description.as_str()
    ) {
        assert_eq!(binary_package, binary.name().unwrap());
        binary.set_multi_arch(Some(debian_control::control::MultiArch::Same));
        binary.set_architecture(Some("any"));
        Some("Add Multi-Arch: same and set Architecture: any.".to_string())
    } else {
        log::warn!("Unable to parse ma-workaround hint: {:?}", hint.description);
        None
    }
}

struct Applier {
    kind: &'static str,
    certainty: Certainty,
    cb: fn(&mut Binary, &Hint) -> Option<String>,
}

lazy_static! {
    static ref APPLIERS: Vec<Applier> = vec![
        Applier {
            kind: "ma-foreign",
            certainty: Certainty::Certain,
            cb: apply_hint_ma_foreign,
        },
        Applier {
            kind: "file-conflict",
            certainty: Certainty::Certain,
            cb: apply_hint_file_conflict,
        },
        Applier {
            kind: "ma-foreign-library",
            certainty: Certainty::Certain,
            cb: apply_hint_ma_foreign_lib,
        },
        Applier {
            kind: "dep-any",
            certainty: Certainty::Certain,
            cb: apply_hint_dep_any,
        },
        Applier {
            kind: "ma-same",
            certainty: Certainty::Certain,
            cb: apply_hint_ma_same,
        },
        Applier {
            kind: "arch-all",
            certainty: Certainty::Possible,
            cb: apply_hint_arch_all,
        },
        Applier {
            kind: "ma-workaround",
            certainty: Certainty::Certain,
            cb: apply_hint_ma_workaround,
        },
    ];
}

fn find_applier(kind: &str) -> Option<&'static Applier> {
    APPLIERS.iter().find(|x| x.kind == kind)
}

fn changes_by_description(changes: &[Change]) -> HashMap<String, Vec<String>> {
    let mut by_description = HashMap::new();
    for change in changes {
        by_description
            .entry(change.description.clone())
            .or_insert_with(Vec::new)
            .push(change.binary.clone());
    }
    by_description
}

#[derive(Debug)]
pub enum OverallError {
    BrzError(Error),
    NotDebianPackage(std::path::PathBuf),
    Other(String),
    Python(pyo3::PyErr),
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
            debian_analyzer::editor::EditorError::TemplateError(p, e) => {
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
            OverallError::Python(e) => write!(f, "{}", e),
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
            Error::Other(e) => OverallError::Python(e),
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

pub fn apply_multiarch_hints(
    local_tree: &WorkingTree,
    subpath: &std::path::Path,
    hints: &HashMap<&str, Vec<&Hint>>,
    minimum_certainty: Option<Certainty>,
    committer: Option<String>,
    dirty_tracker: Option<&mut DirtyTreeTracker>,
    update_changelog: bool,
    allow_reformatting: Option<bool>,
) -> Result<OverallResult, OverallError> {
    let minimum_certainty = minimum_certainty.unwrap_or(Certainty::Certain);
    let basis_tree = local_tree.basis_tree().unwrap();
    let (changes, _tree_changes, mut specific_files) = match apply_or_revert(
        local_tree,
        subpath,
        &basis_tree,
        dirty_tracker,
        |path| -> Result<Vec<Change>, OverallError> {
            let mut changes: Vec<Change> = vec![];

            let control_path = path.join("debian/control");

            let mut editor = match TemplatedControlEditor::open(control_path.as_path()) {
                Ok(editor) => editor,
                Err(e) => {
                    return Err(OverallError::Other(e.to_string()));
                }
            };

            for mut binary in editor.binaries() {
                let package = binary.name().unwrap();
                if let Some(hints) = hints.get(package.as_str()) {
                    for hint in hints {
                        let kind = hint.kind();
                        let applier = match find_applier(kind) {
                            Some(applier) => applier,
                            None => {
                                log::warn!("Unknown hint kind: {}", kind);
                                continue;
                            }
                        };
                        if !certainty_sufficient(applier.certainty, Some(minimum_certainty)) {
                            continue;
                        }
                        if let Some(description) = (applier.cb)(&mut binary, hint) {
                            changes.push(Change {
                                binary: binary.name().unwrap(),
                                hint: (*hint).clone(),
                                description,
                                certainty: applier.certainty,
                            });
                        }
                    }
                }
            }

            editor.commit()?;
            Ok(changes)
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

    if update_changelog {
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

    let committer = committer.unwrap_or_else(|| get_committer(local_tree));

    let specific_files_ref = specific_files
        .as_ref()
        .map(|x| x.iter().map(|x| x.as_path()).collect::<Vec<_>>());

    local_tree.commit(
        overall_description.concat().as_str(),
        Some(false),
        Some(&committer),
        specific_files_ref.as_deref(),
    )?;

    Ok(OverallResult { changes })
}
