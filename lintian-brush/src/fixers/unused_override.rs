use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction, OverrideLineSelector};
use crate::lintian_overrides::LintianOverrides;
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

const INTERMITTENT_LINTIAN_TAGS: &[&str] = &["rc-version-greater-than-expected-version"];

#[derive(Debug)]
pub struct UnusedOverride {
    package: String,
    #[cfg_attr(not(feature = "udd"), allow(dead_code))]
    package_type: String,
    tag: String,
    info: String,
}

#[cfg(feature = "udd")]
async fn get_unused_overrides(
    packages: &[(String, String)],
) -> Result<Vec<UnusedOverride>, Box<dyn std::error::Error>> {
    use sqlx::Row;

    let client = debian_analyzer::udd::connect_udd_mirror().await?;

    let mut conditions = Vec::new();
    for (i, _) in packages.iter().enumerate() {
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

    let mut query_builder = sqlx::query(&query);
    for (name, pkg_type) in packages {
        query_builder = query_builder.bind(name).bind(pkg_type);
    }

    let rows = query_builder.fetch_all(&client).await?;

    let mut unused = Vec::new();
    for row in rows {
        let package: String = row.get(0);
        let package_type: String = row.get(1);
        let information: String = row.get(3);
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

fn find_override_files(ws: &dyn FixerWorkspace) -> Result<Vec<PathBuf>, FixerError> {
    let mut paths = Vec::new();
    let source_rel = PathBuf::from("debian/source/lintian-overrides");
    if ws.read_file(&source_rel)?.is_some() {
        paths.push(source_rel);
    }
    if let Some(mut entries) = ws.list_dir(Path::new("debian"))? {
        entries.sort();
        for name in entries {
            if name.ends_with(".lintian-overrides") {
                paths.push(PathBuf::from("debian").join(name));
            }
        }
    }
    Ok(paths)
}

/// Build diagnostics that remove every override line matching one of the
/// `unused_overrides` records. Public so tests can drive the fixer
/// without UDD connectivity.
pub fn detect_with_unused_overrides(
    ws: &dyn FixerWorkspace,
    unused_overrides: &[UnusedOverride],
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut tags_collected: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut pending: Vec<(LintianIssue, OverrideLineSelector, PathBuf)> = Vec::new();

    for rel in find_override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(&content);
        let Ok(overrides) = parsed.ok() else {
            continue;
        };

        for (lineno, line) in overrides.lines().enumerate() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let tag = tag_token.text();
            if INTERMITTENT_LINTIAN_TAGS.contains(&tag) {
                continue;
            }
            let line_info = line.info().unwrap_or_default();
            let package_spec = line.package_spec().and_then(|s| s.package_name());

            for unused in unused_overrides {
                if let Some(ref pkg) = package_spec {
                    if !pkg.contains(&unused.package) {
                        continue;
                    }
                }
                if tag != unused.tag {
                    continue;
                }
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
                if expected_info != actual_info {
                    continue;
                }

                let override_text = actual_info.clone();
                let file_location = format!("[{}:{}]", rel.display(), lineno + 1);
                let issue = LintianIssue::source_with_info(
                    "unused-override",
                    vec![format!("{} {}", override_text, file_location)],
                );
                tags_collected.insert(tag.to_string());
                let info_for_selector = (!line_info.is_empty()).then(|| line_info.clone());
                pending.push((
                    issue,
                    OverrideLineSelector {
                        tag: tag.to_string(),
                        info: info_for_selector,
                        package: package_spec.clone(),
                    },
                    rel.clone(),
                ));
                break;
            }
        }
    }

    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let mut description = format!("{} unused lintian override(s)\n\n", tags_collected.len());
    for tag in &tags_collected {
        description.push_str(&format!("* {}\n", tag));
    }
    let label = format!(
        "Remove {} unused lintian override(s).",
        tags_collected.len()
    );

    for (issue, selector, file) in pending {
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                description.clone(),
                label.clone(),
                vec![Action::LintianOverrides(LintianOverridesAction::DropLine {
                    file,
                    selector,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }
    Ok(diagnostics)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if preferences.diligence.unwrap_or(0) < 1 {
        return Ok(Vec::new());
    }
    if !preferences.net_access.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut packages = Vec::new();
    if let Some(source) = control.source() {
        if let Some(name) = source.name() {
            packages.push((name, "source".to_string()));
        }
    }
    for para in control.binaries() {
        if let Some(name) = para.name() {
            packages.push((name, "binary".to_string()));
        }
    }
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| FixerError::Io(std::io::Error::other(e)))?;
    let unused_overrides = match runtime.block_on(get_unused_overrides(&packages)) {
        Ok(u) => u,
        Err(_) => return Ok(Vec::new()),
    };
    if unused_overrides.is_empty() {
        return Ok(Vec::new());
    }

    detect_with_unused_overrides(ws, &unused_overrides)
}

declare_detector! {
    name: "unused-override",
    tags: ["unused-override"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Source",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        crate::workspace::Trigger::File("debian/source/lintian-overrides"),
        crate::workspace::Trigger::Glob("debian/*.lintian-overrides"),
    ],
    cost: crate::workspace::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::apply_diagnostics;
    use crate::workspace::TreeFixerWorkspace;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run(
        base: &Path,
        unused_overrides: &[UnusedOverride],
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let ws = TreeFixerWorkspace::new(base.to_path_buf(), "test", version);
        let diagnostics = detect_with_unused_overrides(&ws, unused_overrides)?;
        apply_diagnostics(base, &diagnostics, &FixerPreferences::default())
    }

    #[test]
    fn test_remove_unused_override() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "test-package source: some-tag some info\nanother-tag\n",
        )
        .unwrap();

        let unused = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "some-tag".to_string(),
            info: "some info".to_string(),
        }];

        let result = run(tmp.path(), &unused).unwrap();
        assert_eq!(result.description, "Remove 1 unused lintian override(s).");
        let content = fs::read_to_string(&overrides).unwrap();
        assert!(content.contains("another-tag"));
        assert!(!content.contains("some-tag"));
    }

    #[test]
    fn test_no_unused_overrides() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "some-valid-tag\n").unwrap();

        let unused = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "different-tag".to_string(),
            info: "".to_string(),
        }];

        assert!(matches!(
            run(tmp.path(), &unused),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&overrides).unwrap(), "some-valid-tag\n");
    }

    #[test]
    fn test_remove_all_overrides_deletes_file() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(&overrides, "test-package source: unused-tag\n").unwrap();

        let unused = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "unused-tag".to_string(),
            info: "".to_string(),
        }];

        let result = run(tmp.path(), &unused).unwrap();
        assert_eq!(result.description, "Remove 1 unused lintian override(s).");
        assert!(!overrides.exists());
    }

    #[test]
    fn test_no_override_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();

        let unused = vec![UnusedOverride {
            package: "test-package".to_string(),
            package_type: "source".to_string(),
            tag: "some-tag".to_string(),
            info: "".to_string(),
        }];

        assert!(matches!(
            run(tmp.path(), &unused),
            Err(FixerError::NoChanges)
        ));
    }
}
