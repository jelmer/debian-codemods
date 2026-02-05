use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::lintian::StandardsVersion;
use debian_control::lossless::Control;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

// For the Debian Policy upgrade checklist, see
// https://www.debian.org/doc/debian-policy/upgrading-checklist.html

// Dictionary mapping source and target versions
fn upgrade_path() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("4.1.0", "4.1.1");
    map.insert("4.1.4", "4.1.5");
    map.insert("4.2.0", "4.2.1");
    map.insert("4.3.0", "4.4.0");
    map.insert("4.4.0", "4.4.1");
    map.insert("4.4.1", "4.5.0");
    map.insert("4.5.0", "4.5.1");
    map.insert("4.5.1", "4.6.0");
    map.insert("4.6.0", "4.6.1");
    map.insert("4.6.1", "4.6.2");
    map
}

#[derive(Debug)]
enum UpgradeCheckResult {
    Success(Vec<String>),
    Failure { section: String, reason: String },
    Unable { section: String, reason: String },
}

fn check_4_1_1(base_path: &Path) -> UpgradeCheckResult {
    let changelog_path = base_path.join("debian/changelog");
    if !changelog_path.exists() {
        return UpgradeCheckResult::Failure {
            section: "4.4".to_string(),
            reason: "debian/changelog does not exist".to_string(),
        };
    }
    UpgradeCheckResult::Success(vec!["debian/changelog exists".to_string()])
}

fn has_debhelper_compat_in_control(control: &Control) -> bool {
    let Some(source) = control.source() else {
        return false;
    };

    if let Some(build_depends) = source.build_depends() {
        build_depends.entries().any(|entry| {
            entry
                .relations()
                .any(|rel| rel.name() == "debhelper-compat")
        })
    } else {
        false
    }
}

fn check_4_4_0(base_path: &Path) -> UpgradeCheckResult {
    // Check that the package uses debhelper
    if base_path.join("debian/compat").exists() {
        return UpgradeCheckResult::Success(vec!["package uses debhelper".to_string()]);
    }

    let control_path = base_path.join("debian/control");
    let Ok(content) = std::fs::read_to_string(&control_path) else {
        return UpgradeCheckResult::Failure {
            section: "4.9".to_string(),
            reason: "package does not use dh".to_string(),
        };
    };

    let Ok(control) = Control::from_str(&content) else {
        return UpgradeCheckResult::Failure {
            section: "4.9".to_string(),
            reason: "package does not use dh".to_string(),
        };
    };

    if has_debhelper_compat_in_control(&control) {
        return UpgradeCheckResult::Success(vec!["package uses debhelper".to_string()]);
    }

    UpgradeCheckResult::Failure {
        section: "4.9".to_string(),
        reason: "package does not use dh".to_string(),
    }
}

fn count_vcs_fields(source: &debian_control::lossless::Source) -> usize {
    // Iterate over all fields and count those starting with "Vcs-" (excluding "Vcs-Browser")
    source
        .as_deb822()
        .items()
        .filter(|(name, _)| {
            let name_lower = name.to_lowercase();
            name_lower != "vcs-browser" && name_lower.starts_with("vcs-")
        })
        .count()
}

fn check_copyright_files_not_directories(base_path: &Path) -> Result<(), String> {
    let copyright_path = base_path.join("debian/copyright");
    if !copyright_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&copyright_path)
        .map_err(|_| "cannot read copyright".to_string())?;

    let copyright: debian_copyright::lossless::Copyright = content
        .parse()
        .map_err(|_| "not machine-readable".to_string())?;

    for para in copyright.iter_files() {
        for glob in para.files() {
            let file_path = base_path.join(&glob);
            if file_path.is_dir() {
                return Err(
                    "Wildcards are required to match the contents of directories".to_string(),
                );
            }
        }
    }

    Ok(())
}

fn check_4_4_1(base_path: &Path) -> UpgradeCheckResult {
    let mut results = Vec::new();

    // Check that there is only one Vcs field
    let control_path = base_path.join("debian/control");
    let Ok(content) = std::fs::read_to_string(&control_path) else {
        return UpgradeCheckResult::Success(results);
    };

    let Ok(control) = Control::from_str(&content) else {
        return UpgradeCheckResult::Success(results);
    };

    if let Some(source) = control.source() {
        let vcs_count = count_vcs_fields(&source);

        if vcs_count > 1 {
            return UpgradeCheckResult::Failure {
                section: "5.6.26".to_string(),
                reason: "package has more than one Vcs-<type> field".to_string(),
            };
        } else if vcs_count == 0 {
            results.push("package has no Vcs-<type> fields".to_string());
        } else {
            results.push("package has only one Vcs-<type> field".to_string());
        }
    }

    // Check that Files entries don't refer to directories
    if let Err(reason) = check_copyright_files_not_directories(base_path) {
        return UpgradeCheckResult::Failure {
            section: "copyright-format".to_string(),
            reason,
        };
    }

    results.push("Files entries in debian/copyright don't refer to directories".to_string());
    UpgradeCheckResult::Success(results)
}

fn check_changelog_epoch_changes(base_path: &Path) -> bool {
    let changelog_path = base_path.join("debian/changelog");
    let Ok(content) = std::fs::read_to_string(&changelog_path) else {
        return false;
    };

    let Ok(cl) = content.parse::<debian_changelog::ChangeLog>() else {
        return false;
    };

    let mut epochs = std::collections::HashSet::new();
    for entry in cl.iter().take(2) {
        if let Some(version) = entry.version() {
            let epoch = version.epoch.unwrap_or(0);
            epochs.insert(epoch);
        }
        // Skip entries without versions
    }

    epochs.len() > 1
}

fn check_4_1_5(base_path: &Path) -> UpgradeCheckResult {
    if check_changelog_epoch_changes(base_path) {
        return UpgradeCheckResult::Unable {
            section: "5.6.12".to_string(),
            reason: "last release changes epoch".to_string(),
        };
    }

    UpgradeCheckResult::Success(vec!["Package did not recently introduce epoch".to_string()])
}

fn poor_grep(path: &Path, needle: &[u8]) -> bool {
    if let Ok(content) = std::fs::read(path) {
        content.windows(needle.len()).any(|window| window == needle)
    } else {
        false
    }
}

fn check_maintainer_scripts_for_users(debian_dir: &Path) -> Result<bool, UpgradeCheckResult> {
    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return Ok(false);
    };

    let mut uses_update_rc_d = false;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.ends_with(".postinst") && !name_str.ends_with(".preinst") {
            continue;
        }

        let path = entry.path();
        if poor_grep(&path, b"adduser") || poor_grep(&path, b"useradd") {
            return Err(UpgradeCheckResult::Unable {
                section: "9.2.1".to_string(),
                reason: "dynamically generated usernames should start with an underscore"
                    .to_string(),
            });
        }

        if poor_grep(&path, b"update-rc.d") {
            uses_update_rc_d = true;
        }
    }

    Ok(uses_update_rc_d)
}

fn check_init_files_have_systemd_units(
    debian_dir: &Path,
    uses_update_rc_d: bool,
) -> Result<(), UpgradeCheckResult> {
    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.ends_with(".init") {
            continue;
        }

        let shortname = &name_str[..name_str.len() - 5];
        let service_path = debian_dir.join(format!("{}.service", shortname));
        let template_service_path = debian_dir.join(format!("{}@.service", shortname));

        if !service_path.exists() && !template_service_path.exists() {
            return Err(UpgradeCheckResult::Failure {
                section: "9.3.1".to_string(),
                reason: "packages that include system services should include systemd units"
                    .to_string(),
            });
        }

        if !uses_update_rc_d {
            return Err(UpgradeCheckResult::Failure {
                section: "9.3.3".to_string(),
                reason: "update-rc usage if required if package includes init script".to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_5_0(base_path: &Path) -> UpgradeCheckResult {
    let debian_dir = base_path.join("debian");
    if !debian_dir.is_dir() {
        return UpgradeCheckResult::Success(vec![
            "Package does not create users".to_string(),
            "Package does not ship init files".to_string(),
        ]);
    }

    let uses_update_rc_d = match check_maintainer_scripts_for_users(&debian_dir) {
        Ok(uses) => uses,
        Err(result) => return result,
    };

    let mut results = vec!["Package does not create users".to_string()];

    if let Err(result) = check_init_files_have_systemd_units(&debian_dir, uses_update_rc_d) {
        return result;
    }

    if uses_update_rc_d {
        results.push(
            "Package does not ship any init files without matching systemd units".to_string(),
        );
        results.push("Package ships init files but uses update-rc.d".to_string());
    } else {
        results.push("Package does not ship init files".to_string());
    }

    UpgradeCheckResult::Success(results)
}

fn check_4_5_1(base_path: &Path) -> UpgradeCheckResult {
    let patches_dir = base_path.join("debian/patches");
    if !patches_dir.is_dir() {
        return UpgradeCheckResult::Success(vec!["Package does not have any patches".to_string()]);
    }

    let Ok(entries) = std::fs::read_dir(&patches_dir) else {
        return UpgradeCheckResult::Success(vec!["Package does not have any patches".to_string()]);
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".series") {
            return UpgradeCheckResult::Failure {
                section: "4.5.1".to_string(),
                reason: "package contains non-default series file".to_string(),
            };
        }
    }

    UpgradeCheckResult::Success(vec![
        "Package does not ship any non-default series files".to_string()
    ])
}

fn check_4_2_1(_base_path: &Path) -> UpgradeCheckResult {
    UpgradeCheckResult::Success(vec![])
}

fn check_for_lib64_references(debian_dir: &Path) -> Result<(), UpgradeCheckResult> {
    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        if poor_grep(&entry.path(), b"lib64") {
            return Err(UpgradeCheckResult::Unable {
                section: "9.1.1".to_string(),
                reason: "unable to verify whether package install files into /usr/lib/64"
                    .to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_6_0(base_path: &Path) -> UpgradeCheckResult {
    let debian_dir = base_path.join("debian");
    if !debian_dir.is_dir() {
        return UpgradeCheckResult::Success(vec![
            "Package does not contain any references to lib64".to_string(),
        ]);
    }

    if let Err(result) = check_for_lib64_references(&debian_dir) {
        return result;
    }

    UpgradeCheckResult::Success(vec![
        "Package does not contain any references to lib64".to_string()
    ])
}

fn check_4_6_1(_base_path: &Path) -> UpgradeCheckResult {
    // 9.1.1: Restore permission for packages for non-64-bit architectures to
    // install files to /usr/lib64/.
    // -> No need to check anything.
    UpgradeCheckResult::Success(vec![])
}

fn check_for_x_window_manager(debian_dir: &Path) -> Result<(), UpgradeCheckResult> {
    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        if poor_grep(&entry.path(), b"x-window-manager") {
            return Err(UpgradeCheckResult::Unable {
                section: "11.8.4".to_string(),
                reason: "unable to verify priority for /usr/bin/x-window-manager alternative"
                    .to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_6_2(base_path: &Path) -> UpgradeCheckResult {
    let debian_dir = base_path.join("debian");
    if !debian_dir.is_dir() {
        return UpgradeCheckResult::Success(vec![
            "Package does not provide x-window-manager alternative".to_string(),
        ]);
    }

    if let Err(result) = check_for_x_window_manager(&debian_dir) {
        return result;
    }

    UpgradeCheckResult::Success(vec![
        "Package does not provide x-window-manager alternative".to_string(),
    ])
}

fn get_check_fn(version: &str) -> Option<fn(&Path) -> UpgradeCheckResult> {
    match version {
        "4.1.1" => Some(check_4_1_1),
        "4.2.1" => Some(check_4_2_1),
        "4.4.0" => Some(check_4_4_0),
        "4.4.1" => Some(check_4_4_1),
        "4.1.5" => Some(check_4_1_5),
        "4.5.0" => Some(check_4_5_0),
        "4.5.1" => Some(check_4_5_1),
        "4.6.0" => Some(check_4_6_0),
        "4.6.1" => Some(check_4_6_1),
        "4.6.2" => Some(check_4_6_2),
        _ => None,
    }
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Check if this is a debcargo package - debcargo packages manage their own control files
    if base_path.join("debian/debcargo.toml").exists() {
        return Err(FixerError::NoChanges);
    }

    let control_path = base_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let mut source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    let current_version_str = match source.standards_version() {
        Some(sv) => {
            tracing::debug!("Current standards version: {}", sv);
            sv
        }
        None => {
            tracing::debug!("No standards version found");
            return Err(FixerError::NoChanges);
        }
    };

    // Get all valid standards versions and find the latest
    let standards_versions_opt = debian_analyzer::lintian::iter_standards_versions_opt();

    let (latest_version, current_date, _latest_date, tag) = if let Some(iter) =
        standards_versions_opt
    {
        tracing::debug!("Got standards versions iterator");

        // Collect all releases into a Vec for lookup
        let releases: Vec<(StandardsVersion, chrono::DateTime<chrono::Utc>)> = iter
            .map(|release| (release.version, release.timestamp))
            .collect();

        // Parse current version
        let current_version: StandardsVersion = match current_version_str.parse() {
            Ok(sv) => {
                tracing::debug!("Parsed current version: {:?}", sv);
                sv
            }
            Err(e) => {
                tracing::debug!(
                    "Failed to parse current version '{}': {:?}",
                    current_version_str,
                    e
                );
                return Err(FixerError::NoChanges);
            }
        };

        let current_date = releases
            .iter()
            .find(|(v, _)| v == &current_version)
            .map(|(_, d)| *d);
        let latest = releases.iter().map(|(v, _)| v).max().cloned();
        let latest_date = latest
            .as_ref()
            .and_then(|lv| releases.iter().find(|(v, _)| v == lv))
            .map(|(_, d)| *d);

        if let Some(ref latest_ver) = latest {
            if &current_version >= latest_ver {
                // Already at latest or newer
                return Err(FixerError::NoChanges);
            }
        }

        // Determine tag based on age
        let tag = if let (Some(ref curr_date), Some(ref last_date)) = (current_date, latest_date) {
            let age = last_date.signed_duration_since(curr_date);
            if age.num_days() > 365 * 2 {
                "ancient-standards-version"
            } else {
                "out-of-date-standards-version"
            }
        } else {
            "out-of-date-standards-version"
        };

        (latest, current_date, latest_date, tag)
    } else {
        tracing::debug!("No standards versions iterator available");
        // Like Python, continue with None values
        let _current_version: StandardsVersion = match current_version_str.parse() {
            Ok(sv) => sv,
            Err(_) => return Err(FixerError::NoChanges),
        };
        (None, None, None, "out-of-date-standards-version")
    };

    // Build info string like Python: "4.1.0 (released 2017-07-04) (current is 4.6.2)"
    let mut info_parts = vec![current_version_str.clone()];
    if let Some(date) = current_date {
        info_parts.push(format!("(released {})", date.format("%Y-%m-%d")));
    }
    if let Some(ref latest) = latest_version {
        info_parts.push(format!("(current is {})", latest));
    }
    let info_str = info_parts.join(" ");

    let issue = LintianIssue::source_with_info(tag, vec![info_str]);

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Now try to upgrade through the path
    let mut current = current_version_str.clone();
    let path = upgrade_path();

    while let Some(&target) = path.get(current.as_str()) {
        if let Some(check_fn) = get_check_fn(target) {
            match check_fn(base_path) {
                UpgradeCheckResult::Success(_reasons) => {
                    current = target.to_string();
                }
                UpgradeCheckResult::Failure { section, reason } => {
                    tracing::info!(
                        "Upgrade checklist validation from standards {} ⇒ {} failed: {}: {}",
                        current,
                        target,
                        section,
                        reason
                    );
                    break;
                }
                UpgradeCheckResult::Unable { section, reason } => {
                    tracing::info!(
                        "Unable to validate checklist from standards {} ⇒ {}: {}: {}",
                        current,
                        target,
                        section,
                        reason
                    );
                    break;
                }
            }
        } else {
            // No check function for this version, just upgrade
            current = target.to_string();
        }
    }

    // If we didn't upgrade at all, return no changes
    if current == current_version_str {
        return Err(FixerError::NoChanges);
    }

    // Update the control file
    source.set("Standards-Version", &current);
    std::fs::write(&control_path, control.to_string())?;

    Ok(FixerResult::builder(format!(
        "Update standards version to {}, no changes needed.",
        current
    ))
    .certainty(crate::Certainty::Certain)
    .fixed_issues(vec![issue])
    .build())
}

declare_fixer! {
    name: "out-of-date-standards-version",
    tags: ["out-of-date-standards-version", "ancient-standards-version"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}
