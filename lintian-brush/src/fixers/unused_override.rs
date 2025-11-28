use crate::lintian_overrides::{filter_overrides, LintianOverrides};
use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use debian_control::Control;
use std::fs;
use std::path::{Path, PathBuf};

const INTERMITTENT_LINTIAN_TAGS: &[&str] = &["rc-version-greater-than-expected-version"];

const DEFAULT_UDD_URL: &str = "postgresql://udd-mirror:udd-mirror@udd-mirror.debian.net/udd";

#[derive(Debug)]
struct UnusedOverride {
    package: String,
    package_type: String,
    tag: String,
    info: String,
}

fn find_override_files(base_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        paths.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    if let Ok(entries) = fs::read_dir(&debian_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name() {
                if name.to_string_lossy().ends_with(".lintian-overrides") {
                    paths.push(path);
                }
            }
        }
    }

    paths
}

#[cfg(feature = "udd")]
async fn get_unused_overrides(
    packages: &[(String, String)],
) -> Result<Vec<UnusedOverride>, Box<dyn std::error::Error>> {
    use tokio_postgres::NoTls;

    let udd_url = std::env::var("UDD_URL").unwrap_or_else(|_| DEFAULT_UDD_URL.to_string());

    let (client, connection) = tokio_postgres::connect(&udd_url, NoTls).await?;

    // Spawn the connection in the background
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    // Build the query
    let mut conditions = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();

    for (i, (name, pkg_type)) in packages.iter().enumerate() {
        let param_idx = i * 2 + 1;
        conditions.push(format!(
            "(package = ${} AND package_type = ${})",
            param_idx,
            param_idx + 1
        ));
    }

    let query = format!(
        "SELECT package, package_type, package_version, information
         FROM lintian
         WHERE tag = 'unused-override' AND ({})",
        conditions.join(" OR ")
    );

    // Add parameters
    let mut param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    for (name, pkg_type) in packages {
        param_refs.push(name);
        param_refs.push(pkg_type);
    }

    let rows = client.query(&query, &param_refs[..]).await?;

    let mut unused = Vec::new();
    for row in rows {
        let package: String = row.get(0);
        let package_type: String = row.get(1);
        // let package_version: String = row.get(2);
        let information: String = row.get(3);

        // Parse information to get tag and info
        // Format is typically "tag info" or just "tag"
        let parts: Vec<&str> = information.splitn(2, ' ').collect();
        let tag = parts[0].to_string();
        let info = if parts.len() > 1 {
            parts[1].to_string()
        } else {
            String::new()
        };

        unused.push(UnusedOverride {
            package,
            package_type,
            tag,
            info,
        });
    }

    Ok(unused)
}

#[cfg(not(feature = "udd"))]
async fn get_unused_overrides(
    _packages: &[(String, String)],
) -> Result<Vec<UnusedOverride>, Box<dyn std::error::Error>> {
    Err("UDD support not compiled in. Rebuild with --features udd".into())
}

fn process_overrides_file(
    path: &Path,
    unused_overrides: &[UnusedOverride],
    ignore_tags: &[&str],
) -> Result<(bool, Vec<String>), FixerError> {
    let content = fs::read_to_string(path)?;
    let parsed = LintianOverrides::parse(&content);
    let overrides = parsed.ok().map_err(|_| FixerError::NoChanges)?;

    let mut removed_tags = Vec::new();

    // Filter out unused overrides
    let filtered = filter_overrides(&overrides, |line| {
        // Always keep comments and empty lines
        if line.is_comment() || line.is_empty() {
            return true;
        }

        if let Some(tag_token) = line.tag() {
            let tag = tag_token.text();

            // Skip if tag is in ignore list
            if ignore_tags.contains(&tag) {
                return true;
            }

            // Check if this override is unused
            let line_info = line.info().unwrap_or_default();
            let package_spec = line.package_spec();

            for unused in unused_overrides {
                // Match package if specified
                if let Some(pkg_spec) = &package_spec {
                    if let Some(pkg_name) = pkg_spec.package_name() {
                        // Package spec might contain both package name and type like "package-name source"
                        // For now, just check if the package name matches
                        if !pkg_name.contains(&unused.package) {
                            continue;
                        }
                    }
                }

                // Match tag
                if tag != unused.tag {
                    continue;
                }

                // Match info
                let expected_info = if unused.info.is_empty() {
                    tag.to_string()
                } else {
                    format!("{} {}", tag, unused.info)
                };

                let actual_info = if line_info.is_empty() {
                    tag.to_string()
                } else {
                    format!("{} {}", tag, line_info)
                };

                if expected_info == actual_info {
                    // This override is unused, remove it
                    if !removed_tags.contains(&tag.to_string()) {
                        removed_tags.push(tag.to_string());
                    }
                    return false;
                }
            }
        }

        true // Keep this line
    });

    let changed = !removed_tags.is_empty();

    if changed {
        let new_content = filtered.text();
        if new_content.trim().is_empty() {
            // If the file is now empty, delete it
            fs::remove_file(path)?;
        } else {
            fs::write(path, new_content)?;
        }
    }

    Ok((changed, removed_tags))
}

/// Remove unused overrides from override files given a list of unused overrides
/// This is the testable core function that doesn't require UDD connectivity
pub fn remove_unused_overrides_from_files(
    base_path: &Path,
    unused_overrides: &[UnusedOverride],
) -> Result<FixerResult, FixerError> {
    let override_files = find_override_files(base_path);

    if override_files.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut all_removed_tags = Vec::new();
    let mut any_changed = false;

    for path in override_files {
        match process_overrides_file(&path, unused_overrides, INTERMITTENT_LINTIAN_TAGS) {
            Ok((changed, removed_tags)) => {
                if changed {
                    any_changed = true;
                    for tag in removed_tags {
                        if !all_removed_tags.contains(&tag) {
                            all_removed_tags.push(tag);
                        }
                    }
                }
            }
            Err(e) => {
                // If it's a not-found or permission error, just skip this file
                if matches!(e, FixerError::Io(_)) {
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    if !any_changed {
        return Err(FixerError::NoChanges);
    }

    let mut description = format!(
        "Remove {} unused lintian overrides.\n\n",
        all_removed_tags.len()
    );
    for tag in &all_removed_tags {
        description.push_str(&format!("* {}\n", tag));
    }

    Ok(FixerResult::builder(&description)
        .fixed_tags(vec!["unused-override"])
        .certainty(crate::Certainty::Certain)
        .build())
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    // Check diligence level (mimics "if diligence() < 1")
    if preferences.diligence.unwrap_or(0) < 1 {
        // Python exits with 0 in this case
        return Err(FixerError::NoChanges);
    }

    // Check net access (mimics "if not net_access_allowed()")
    if !preferences.net_access.unwrap_or(false) {
        // Python exits with 0 in this case
        return Err(FixerError::NoChanges);
    }

    // Read debian/control to get package names
    let control_path = base_path.join("debian/control");
    let control = Control::from_file(&control_path)
        .map_err(|e| FixerError::Io(std::io::Error::other(e.to_string())))?;

    let mut packages = Vec::new();

    // Add source package
    if let Some(source) = control.source() {
        if let Some(source_name) = source.name() {
            packages.push((source_name, "source".to_string()));
        }
    }

    // Add binary packages
    for para in control.binaries() {
        if let Some(package_name) = para.name() {
            packages.push((package_name, "binary".to_string()));
        }
    }

    if packages.is_empty() {
        return Err(FixerError::Other(
            "No packages found in debian/control".to_string(),
        ));
    }

    // Query UDD for unused overrides (this requires tokio runtime)
    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| FixerError::Io(std::io::Error::other(e)))?;

    let unused_overrides = runtime
        .block_on(get_unused_overrides(&packages))
        .map_err(|e| {
            #[cfg(not(feature = "udd"))]
            return FixerError::Other(e.to_string());

            #[cfg(feature = "udd")]
            FixerError::Other(e.to_string())
        })?;

    if unused_overrides.is_empty() {
        return Err(FixerError::NoChanges);
    }

    remove_unused_overrides_from_files(base_path, &unused_overrides)
}

declare_fixer! {
    name: "unused-override",
    tags: ["unused-override"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_remove_unused_override() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "test-package source: some-tag some info\nanother-tag\n",
        )
        .unwrap();

        let unused_overrides = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "some-tag".to_string(),
            info: "some info".to_string(),
        }];

        let result = remove_unused_overrides_from_files(base_path, &unused_overrides).unwrap();
        assert!(result
            .description
            .contains("Remove 1 unused lintian overrides"));
        assert!(result.description.contains("* some-tag"));

        // File should exist with only the valid tag
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert!(content.contains("another-tag"));
        assert!(!content.contains("some-tag"));
    }

    #[test]
    fn test_no_unused_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, "some-valid-tag\n").unwrap();

        let unused_overrides = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "different-tag".to_string(),
            info: "".to_string(),
        }];

        let result = remove_unused_overrides_from_files(base_path, &unused_overrides);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // File should still exist with original content
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert_eq!(content, "some-valid-tag\n");
    }

    #[test]
    fn test_remove_all_overrides_deletes_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(&overrides_path, "test-package source: unused-tag\n").unwrap();

        let unused_overrides = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "unused-tag".to_string(),
            info: "".to_string(),
        }];

        let result = remove_unused_overrides_from_files(base_path, &unused_overrides).unwrap();
        assert!(result.description.contains("unused-tag"));

        // File should be deleted since it's now empty
        assert!(!overrides_path.exists());
    }

    #[test]
    fn test_no_override_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let unused_overrides = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "some-tag".to_string(),
            info: "".to_string(),
        }];

        let result = remove_unused_overrides_from_files(base_path, &unused_overrides);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
