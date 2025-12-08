use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue, PackageType};
use debian_control::lossless::relations::Relations;
use regex::bytes::Regex;
use std::path::Path;

fn previous_release(release: &str) -> Option<String> {
    use chrono::Utc;
    use distro_info::{DebianDistroInfo, DistroInfo};

    let debian = DebianDistroInfo::new().ok()?;
    let today = Utc::now().date_naive();

    // Handle special cases for development releases
    if release == "experimental" || release == "unstable" || release == "sid" {
        // Find the latest stable release (released and still supported)
        let supported = debian.supported(today);
        let stable = supported
            .iter()
            .filter(|r| r.released_at(today))
            .max_by_key(|r| r.release())?;
        return Some(stable.series().to_string());
    }

    // For testing, return stable
    let releases = debian.releases();
    let testing = releases
        .iter()
        .filter(|r| r.created_at(today) && !r.released_at(today))
        .max_by_key(|r| r.created())?;
    if release == testing.series() {
        let supported = debian.supported(today);
        let stable = supported
            .iter()
            .filter(|r| r.released_at(today))
            .max_by_key(|r| r.release())?;
        return Some(stable.series().to_string());
    }

    // Get all releases and find the previous one
    let releases = debian.releases();
    if let Some(idx) = releases.iter().position(|r| r.series() == release) {
        if idx > 0 {
            return Some(releases[idx - 1].series().to_string());
        }
    }

    None
}

#[cfg(feature = "udd")]
async fn package_exists_udd(
    package: &str,
    release: &str,
    version_info: Option<(&str, String)>,
) -> Result<bool, Box<dyn std::error::Error>> {
    use debian_analyzer::udd::connect_udd_mirror;

    let pool = connect_udd_mirror().await?;

    let mut query = "SELECT TRUE FROM packages WHERE release = $1 AND package = $2".to_string();
    let mut bind_count = 2;

    if let Some((op, ref _version)) = version_info {
        bind_count += 1;
        let sql_op = match op {
            "=" => "=",
            ">=" => ">=",
            "<=" => "<=",
            ">>" => ">",
            "<<" => "<",
            _ => return Ok(false),
        };
        query.push_str(&format!(" AND version {} ${}", sql_op, bind_count));
    }

    if let Some((_op, ref version)) = version_info {
        let row: Option<(bool,)> = sqlx::query_as(&query)
            .bind(release)
            .bind(package)
            .bind(version)
            .fetch_optional(&pool)
            .await?;
        Ok(row.is_some())
    } else {
        let row: Option<(bool,)> = sqlx::query_as(&query)
            .bind(release)
            .bind(package)
            .fetch_optional(&pool)
            .await?;
        Ok(row.is_some())
    }
}

fn package_exists(
    package: &str,
    release: &str,
    version_info: Option<(&str, String)>,
    preferences: &FixerPreferences,
) -> Option<bool> {
    // Check environment variable first (for testing without network)
    if !preferences.net_access.unwrap_or(true) {
        let env_var_name = format!("{}_PACKAGES", release.to_uppercase());

        // Check preferences.extra_env first (for in-process Rust fixers in tests)
        let packages_env_str = if let Some(extra_env) = &preferences.extra_env {
            extra_env.get(&env_var_name).cloned()
        } else {
            None
        }
        .or_else(|| std::env::var(&env_var_name).ok());

        if let Some(packages_env) = packages_env_str {
            return Some(packages_env.split(',').any(|p| p == package));
        }
        return None;
    }

    // Try UDD if network access is allowed and udd feature is enabled
    #[cfg(feature = "udd")]
    {
        let rt = tokio::runtime::Runtime::new().ok()?;
        rt.block_on(package_exists_udd(package, release, version_info))
            .ok()
    }

    #[cfg(not(feature = "udd"))]
    {
        let _ = (package, release, version_info);
        None
    }
}

fn migration_done(rels: &Relations, preferences: &FixerPreferences) -> bool {
    let compat_release = preferences.compat_release.as_deref().unwrap_or("unstable");
    let previous = match previous_release(compat_release) {
        Some(p) => p,
        None => return false, // Can't determine if migration is done
    };

    for rel_or in rels.entries() {
        let relations: Vec<_> = rel_or.relations().collect();

        if relations.len() > 1 {
            // Not sure how to handle | Replaces
            return false;
        }

        for rel in relations {
            let version_info = rel.version().map(|(op, ver)| {
                let op_str = match op {
                    debian_control::relations::VersionConstraint::GreaterThanEqual => ">=",
                    debian_control::relations::VersionConstraint::LessThanEqual => "<=",
                    debian_control::relations::VersionConstraint::GreaterThan => ">>",
                    debian_control::relations::VersionConstraint::LessThan => "<<",
                    debian_control::relations::VersionConstraint::Equal => "=",
                };
                (op_str, ver.to_string())
            });

            // If package might still exist in previous release, migration not done
            if package_exists(&rel.name(), &previous, version_info, preferences) != Some(false) {
                return false;
            }
        }
    }

    true
}

fn eliminate_dbgsym_migration(
    line: &[u8],
    line_no: usize,
    basedir: &Path,
    preferences: &FixerPreferences,
    fixed_issues: &mut Vec<LintianIssue>,
    overridden_issues: &mut Vec<LintianIssue>,
) -> Vec<u8> {
    if !line.starts_with(b"dh_strip") {
        return line.to_vec();
    }

    let re = Regex::new(r#"([ \t]+)--dbgsym-migration[= ]('[^']+'|"[^"]+"|[^ ]+)"#).unwrap();

    let result = re
        .replace_all(line, |caps: &regex::bytes::Captures| {
            let migration_arg = caps.get(2).unwrap().as_bytes();
            let stripped = migration_arg
                .strip_prefix(b"'")
                .and_then(|s| s.strip_suffix(b"'"))
                .or_else(|| {
                    migration_arg
                        .strip_prefix(b"\"")
                        .and_then(|s| s.strip_suffix(b"\""))
                })
                .unwrap_or(migration_arg);

            let stripped_str = match std::str::from_utf8(stripped) {
                Ok(s) => s,
                Err(_) => return caps.get(0).unwrap().as_bytes().to_vec(),
            };

            // Check for variables - too complicated
            if stripped_str.contains('$') {
                return caps.get(0).unwrap().as_bytes().to_vec();
            }

            // Parse the relations
            let (rels, _errors) = Relations::parse_relaxed(stripped_str, true);
            let is_done = migration_done(&rels, preferences);
            if is_done {
                let issue = LintianIssue {
                    package: None,
                    package_type: Some(PackageType::Source),
                    tag: Some("debug-symbol-migration-possibly-complete".to_string()),
                    info: Some(vec![format!(
                        "{} [debian/rules:{}]",
                        String::from_utf8_lossy(caps.get(0).unwrap().as_bytes()).trim(),
                        line_no
                    )]),
                };

                if issue.should_fix(basedir) {
                    fixed_issues.push(issue);
                    return b"".to_vec();
                } else {
                    overridden_issues.push(issue);
                }
            }
            caps.get(0).unwrap().as_bytes().to_vec()
        })
        .to_vec();

    // Handle case where we end up with "dh_strip || dh_strip"
    if result == b"dh_strip || dh_strip" {
        b"dh_strip".to_vec()
    } else {
        result
    }
}

pub fn run(
    basedir: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let rules_path = basedir.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::ScriptNotFound(rules_path));
    }

    let content = std::fs::read(&rules_path)?;
    let mut makefile = makefile_lossless::Makefile::read_relaxed(content.as_slice())
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/rules: {}", e)))?;

    let mut made_changes = false;
    let mut rules_to_check: Vec<usize> = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for (rule_idx, mut rule) in makefile.rules().enumerate() {
        let mut commands_to_update = Vec::new();
        let mut commands_to_remove = Vec::new();

        for (i, recipe_node) in rule.recipe_nodes().enumerate() {
            let recipe = recipe_node.text();
            let line_no = recipe_node.line() + 1;
            let ret = eliminate_dbgsym_migration(
                recipe.as_bytes(),
                line_no,
                basedir,
                preferences,
                &mut fixed_issues,
                &mut overridden_issues,
            );
            if ret.is_empty() {
                // Command should be removed
                commands_to_remove.push(i);
            } else if ret != recipe.as_bytes() {
                // Command should be updated
                commands_to_update.push((i, String::from_utf8_lossy(&ret).into_owned()));
            }
        }

        // Apply updates
        for (i, new_recipe) in &commands_to_update {
            rule.replace_command(*i, new_recipe);
            made_changes = true;
        }

        // Remove commands in reverse order to maintain indices
        for i in commands_to_remove.iter().rev() {
            rule.remove_command(*i);
            made_changes = true;
        }

        // Track rules that need discard_pointless_override check
        if !commands_to_update.is_empty() || !commands_to_remove.is_empty() {
            rules_to_check.push(rule_idx);
        }
    }

    // Discard pointless overrides for modified rules
    let all_rules: Vec<_> = makefile.rules().collect();
    for rule_idx in rules_to_check {
        if let Some(rule) = all_rules.get(rule_idx) {
            debian_analyzer::rules::discard_pointless_override(&mut makefile, rule);
        }
    }

    if !made_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    std::fs::write(&rules_path, makefile.to_string())?;

    Ok(
        FixerResult::builder("Drop transition for old debug package migration.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

declare_fixer! {
    name: "debug-symbol-migration-possibly-complete",
    tags: ["debug-symbol-migration-possibly-complete"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}
