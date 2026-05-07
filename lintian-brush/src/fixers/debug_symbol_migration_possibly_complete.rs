use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType};
use debian_control::lossless::relations::Relations;
use regex::bytes::Regex;
use std::path::PathBuf;

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
            let Some(name) = rel.try_name() else {
                return false;
            };
            if package_exists(&name, &previous, version_info, preferences) != Some(false) {
                return false;
            }
        }
    }

    true
}

/// Result of analyzing a recipe line: the rewritten text (if it should
/// change) and the lintian issue reported for the eliminated migration.
struct RecipeRewrite {
    new_text: Vec<u8>,
    issue: LintianIssue,
}

fn eliminate_dbgsym_migration(
    line: &[u8],
    line_no: usize,
    preferences: &FixerPreferences,
) -> Option<RecipeRewrite> {
    if !line.starts_with(b"dh_strip") {
        return None;
    }

    let re = Regex::new(r#"([ \t]+)--dbgsym-migration[= ]('[^']+'|"[^"]+"|[^ ]+)"#).unwrap();

    let mut matched_text: Option<Vec<u8>> = None;
    let mut any_eliminated = false;
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
            if stripped_str.contains('$') {
                return caps.get(0).unwrap().as_bytes().to_vec();
            }

            let (rels, _errors) = Relations::parse_relaxed(stripped_str, true);
            if migration_done(&rels, preferences) {
                if matched_text.is_none() {
                    matched_text = Some(caps.get(0).unwrap().as_bytes().to_vec());
                }
                any_eliminated = true;
                return b"".to_vec();
            }
            caps.get(0).unwrap().as_bytes().to_vec()
        })
        .to_vec();

    if !any_eliminated {
        return None;
    }

    // Collapse "dh_strip || dh_strip" → "dh_strip".
    let new_text = if result == b"dh_strip || dh_strip" {
        b"dh_strip".to_vec()
    } else {
        result
    };

    let matched = matched_text.unwrap_or_default();
    let issue = LintianIssue {
        package: None,
        package_type: Some(PackageType::Source),
        tag: Some("debug-symbol-migration-possibly-complete".to_string()),
        info: Some(format!(
            "{} [debian/rules:{}]",
            String::from_utf8_lossy(&matched).trim(),
            line_no
        )),
    };

    Some(RecipeRewrite { new_text, issue })
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for rule in makefile.rules() {
        // Compute the rewrites for each recipe in this rule.
        let mut per_recipe: Vec<(String, Option<RecipeRewrite>)> = Vec::new();
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let line_no = recipe_node.line() + 1;
            let rewrite = eliminate_dbgsym_migration(recipe.as_bytes(), line_no, preferences);
            per_recipe.push((recipe, rewrite));
        }
        // Skip rules with no rewrites.
        if !per_recipe.iter().any(|(_, r)| r.is_some()) {
            continue;
        }

        let primary_target = rule.targets().next().unwrap_or_default();
        let target_str = primary_target.to_string();

        // Compute the post-edit recipes (for detecting pointless overrides).
        let post_edit: Vec<String> = per_recipe
            .iter()
            .map(|(orig, r)| match r {
                Some(rw) => String::from_utf8_lossy(&rw.new_text).to_string(),
                None => orig.clone(),
            })
            .collect();
        let effective: Vec<&String> = post_edit.iter().filter(|l| !l.trim().is_empty()).collect();

        let mut prereqs = rule.prerequisites();
        let has_prereqs = prereqs.next().is_some();
        let is_override = target_str.starts_with("override_");
        let command = if is_override {
            &target_str["override_".len()..]
        } else {
            target_str.as_str()
        };
        let pointless =
            is_override && !has_prereqs && effective.len() == 1 && effective[0].trim() == command;

        if pointless {
            // Drop the entire rule. The first issue (any) carries the
            // tag; subsequent rewrites don't need their own actions
            // because removing the rule subsumes them.
            let issues: Vec<_> = per_recipe
                .iter()
                .filter_map(|(_, r)| r.as_ref().map(|rw| rw.issue.clone()))
                .collect();
            // Emit a single diagnostic for the rule removal, plus one
            // empty-action diagnostic per remaining issue so they all
            // show up in fixed-issues.
            let mut issues_iter = issues.into_iter();
            if let Some(first_issue) = issues_iter.next() {
                diagnostics.push(Diagnostic::with_actions(
                    first_issue,
                    "Debug symbol migration appears complete.",
                    "Drop transition for old debug package migration.",
                    vec![
                        Action::Makefile(MakefileAction::RemoveRule {
                            file: rules_rel.clone(),
                            target: target_str.clone(),
                        }),
                        Action::Makefile(MakefileAction::RemovePhonyTarget {
                            file: rules_rel.clone(),
                            target: target_str.clone(),
                        }),
                    ],
                ));
            }
            for extra_issue in issues_iter {
                diagnostics.push(Diagnostic::with_actions(
                    extra_issue,
                    "Debug symbol migration appears complete.",
                    "Drop transition for old debug package migration.",
                    Vec::new(),
                ));
            }
        } else {
            // Per-recipe rewrites.
            for (orig, r) in per_recipe {
                let Some(rewrite) = r else { continue };
                let new_text_str = String::from_utf8_lossy(&rewrite.new_text).to_string();
                let action = if new_text_str.trim().is_empty() {
                    Action::Makefile(MakefileAction::RemoveRecipe {
                        file: rules_rel.clone(),
                        target: target_str.clone(),
                        recipe: orig.clone(),
                    })
                } else {
                    Action::Makefile(MakefileAction::ReplaceRecipe {
                        file: rules_rel.clone(),
                        target: target_str.clone(),
                        recipe: orig.clone(),
                        new_recipe: new_text_str,
                    })
                };
                diagnostics.push(Diagnostic::with_actions(
                    rewrite.issue,
                    "Debug symbol migration appears complete.",
                    "Drop transition for old debug package migration.",
                    vec![action],
                ));
            }
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(_fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    "Drop transition for old debug package migration.".to_string()
}

declare_detector! {
    name: "debug-symbol-migration-possibly-complete",
    tags: ["debug-symbol-migration-possibly-complete"],
    triggers: [crate::workspace::Trigger::File("debian/rules")],
    cost: crate::workspace::DetectorCost::Network,
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}
