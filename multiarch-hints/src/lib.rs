use lazy_static::lazy_static;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_yaml::from_value;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

pub const MULTIARCH_HINTS_URL: &str = "https://dedup.debian.net/static/multiarch-hints.yaml.xz";
const USER_AGENT: &str = concat!("apply-multiarch-hints/", env!("CARGO_PKG_VERSION"));

const DEFAULT_VALUE_MULTIARCH_HINT: i32 = 100;

lazy_static! {
    static ref MULTIARCH_HINTS_VALUE: HashMap<&'static str, i32> = {
        let mut map = HashMap::new();
        map.insert("ma-foreign", 20);
        map.insert("file-conflict", 50);
        map.insert("ma-foreign-library", 20);
        map.insert("dep-any", 20);
        map.insert("ma-same", 20);
        map.insert("arch-all", 20);
        map
    };
}

pub fn calculate_value(hints: &[&str]) -> i32 {
    hints
        .iter()
        .map(|hint| *MULTIARCH_HINTS_VALUE.get(hint).unwrap_or(&0))
        .sum::<i32>()
        + DEFAULT_VALUE_MULTIARCH_HINT
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

#[derive(Debug, Deserialize)]
pub struct Hint {
    pub binary: String,
    pub description: String,
    pub source: String,
    pub link: String,
    #[serde(deserialize_with = "deserialize_severity")]
    pub severity: Severity,
    pub version: String,
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

pub fn cache_download_multiarch_hints(url: Option<&str>) -> Result<Vec<u8>, Box<dyn Error>> {
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
) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
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
        let mut reader = lzma::read(response)?;
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;
        Ok(Some(buffer))
    } else {
        Ok(Some(response.bytes()?.to_vec()))
    }
}
