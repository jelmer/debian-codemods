// Copyright (C) 2025 Jelmer Vernooĳ <jelmer@jelmer.uk>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

//! Validate and check tag-status.yaml against lintian tags and implemented fixers

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const KNOWN_KEYS: &[&str] = &["tag", "status", "difficulty", "comment"];

#[derive(Parser, Debug)]
#[command(author, version, about = "Validate and check tag-status.yaml", long_about = None)]
struct Args {
    /// List missing tags
    #[arg(long)]
    new_tags: bool,

    /// Check for missing tags and exit with error if any found
    #[arg(long)]
    check: bool,

    /// Report lintian tags that might be good candidates to implement fixers for (requires UDD access)
    #[cfg(feature = "udd")]
    #[arg(long)]
    next: bool,

    /// Comma-separated list of difficulties to exclude (default: hard)
    #[cfg(feature = "udd")]
    #[arg(long, default_value = "hard")]
    exclude: String,
}

fn get_all_lintian_tags() -> Result<HashSet<String>, Box<dyn Error>> {
    let output = Command::new("lintian-explain-tags")
        .arg("--list-tags")
        .output()?;

    if !output.status.success() {
        return Err("lintian-explain-tags failed".into());
    }

    let tags = String::from_utf8(output.stdout)?
        .lines()
        .map(|s| s.to_string())
        .collect();

    Ok(tags)
}

fn get_supported_tags() -> HashSet<String> {
    lintian_brush::builtin_fixers::get_builtin_fixers()
        .iter()
        .flat_map(|fixer| fixer.lintian_tags())
        .map(|s| s.to_string())
        .collect()
}

fn validate_yaml_entry(entry: &serde_yaml::Value) -> Result<(), Box<dyn Error>> {
    let Some(mapping) = entry.as_mapping() else {
        return Ok(());
    };

    let keys: HashSet<String> = mapping
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();

    let known_keys: HashSet<String> = KNOWN_KEYS.iter().map(|s| s.to_string()).collect();
    let extra_keys: Vec<_> = keys.difference(&known_keys).collect();

    if !extra_keys.is_empty() {
        return Err(format!("Unknown keys: {:?}", extra_keys).into());
    }

    Ok(())
}

fn extract_tag_from_entry(entry: &serde_yaml::Value) -> Option<String> {
    let mapping = entry.as_mapping()?;
    let tag_value = mapping.get(&serde_yaml::Value::String("tag".to_string()))?;
    tag_value.as_str().map(|s| s.to_string())
}

fn get_entry_status(entry: &serde_yaml::Value) -> Option<String> {
    let mapping = entry.as_mapping()?;
    let status = mapping.get(&serde_yaml::Value::String("status".to_string()))?;
    status.as_str().map(|s| s.to_string())
}

#[cfg(feature = "udd")]
fn get_entry_difficulty(entry: &serde_yaml::Value) -> String {
    let Some(mapping) = entry.as_mapping() else {
        return "unknown".to_string();
    };

    let Some(difficulty) = mapping.get(&serde_yaml::Value::String("difficulty".to_string())) else {
        return "unknown".to_string();
    };

    difficulty
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn validate_implemented_tags(
    supported_tags: &HashSet<String>,
    per_tag_status: &HashMap<String, serde_yaml::Value>,
) -> Result<(), Box<dyn Error>> {
    for tag in supported_tags {
        let Some(existing) = per_tag_status.get(tag) else {
            continue;
        };

        let Some(status_str) = get_entry_status(existing) else {
            continue;
        };

        if status_str != "implemented" {
            return Err(format!(
                "tag {} is marked as {} in tag-status.yaml, but implemented",
                tag, status_str
            )
            .into());
        }
    }

    Ok(())
}

#[cfg(feature = "udd")]
async fn report_next_tags(
    exclude_difficulties: &[&str],
    per_tag_status: &HashMap<String, serde_yaml::Value>,
    supported_tags: &HashSet<String>,
) -> Result<(), Box<dyn Error>> {
    use sqlx::Row;

    let pool = debian_analyzer::udd::connect_udd_mirror().await?;

    let query = "SELECT tag, COUNT(DISTINCT package) AS package_count, \
                 COUNT(*) AS tag_count FROM lintian \
                 WHERE tag_type NOT IN ('classification') GROUP BY 1 ORDER BY 2 DESC";

    let rows = sqlx::query(query).fetch_all(&pool).await?;

    for row in rows {
        let tag: String = row.get("tag");
        let package_count: i64 = row.get("package_count");
        let tag_count: i64 = row.get("tag_count");

        if supported_tags.contains(&tag) {
            continue;
        }

        let difficulty = per_tag_status
            .get(&tag)
            .map(|entry| get_entry_difficulty(entry))
            .unwrap_or_else(|| "unknown".to_string());

        if exclude_difficulties.contains(&difficulty.as_str()) {
            continue;
        }

        println!("{} {} {}/{}", tag, difficulty, package_count, tag_count);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tag-status.yaml");

    let content = std::fs::read_to_string(&path)?;
    let tag_status: Vec<serde_yaml::Value> = serde_yaml::from_str(&content)?;

    let mut per_tag_status: HashMap<String, serde_yaml::Value> = HashMap::new();
    for entry in &tag_status {
        validate_yaml_entry(entry)?;

        if let Some(tag) = extract_tag_from_entry(entry) {
            per_tag_status.insert(tag, entry.clone());
        }
    }

    let supported_tags = get_supported_tags();
    validate_implemented_tags(&supported_tags, &per_tag_status)?;

    if args.new_tags {
        let all_tags = get_all_lintian_tags()?;
        let mut missing_tags: Vec<_> = all_tags
            .iter()
            .filter(|tag| !per_tag_status.contains_key(*tag))
            .collect();
        missing_tags.sort();

        for tag in missing_tags {
            println!("{}", tag);
        }
    } else if args.check {
        let all_tags = get_all_lintian_tags()?;
        let mut missing_tags: Vec<_> = all_tags
            .iter()
            .filter(|tag| !per_tag_status.contains_key(*tag))
            .collect();
        missing_tags.sort();

        let mut retcode = 0;
        for tag in missing_tags {
            println!("Missing tag: {}", tag);
            retcode = 1;
        }

        std::process::exit(retcode);
    }
    #[cfg(feature = "udd")]
    if args.next {
        let exclude_difficulties: Vec<&str> = args.exclude.split(',').collect();
        report_next_tags(&exclude_difficulties, &per_tag_status, &supported_tags).await?;
    }

    Ok(())
}
