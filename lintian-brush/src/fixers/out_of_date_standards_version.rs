use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use debian_analyzer::lintian::StandardsVersion;
use debian_control::lossless::Control;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    map.insert("4.6.2", "4.7.0");
    map.insert("4.7.0", "4.7.1");
    map.insert("4.7.1", "4.7.2");
    map.insert("4.7.2", "4.7.3");
    map.insert("4.7.3", "4.7.4");
    map.insert("4.7.3", "4.7.4");
    map
}

#[derive(Debug)]
enum UpgradeCheckResult {
    Success(Vec<String>),
    Failure { section: String, reason: String },
    Unable { section: String, reason: String },
}

fn check_4_1_1(_ws: &dyn FixerWorkspace, base_path: &Path) -> UpgradeCheckResult {
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
                .any(|rel| rel.try_name().as_deref() == Some("debhelper-compat"))
        })
    } else {
        false
    }
}

fn check_4_4_0(ws: &dyn FixerWorkspace, base_path: &Path) -> UpgradeCheckResult {
    // Check that the package uses debhelper
    if base_path.join("debian/compat").exists() {
        return UpgradeCheckResult::Success(vec!["package uses debhelper".to_string()]);
    }

    let Ok(control) = ws.parsed_control() else {
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

fn check_copyright_files_not_directories(
    ws: &dyn FixerWorkspace,
    base_path: &Path,
) -> Result<(), String> {
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(()),
        Err(_) => return Err("not machine-readable".to_string()),
    };

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

fn check_4_4_1(ws: &dyn FixerWorkspace, base_path: &Path) -> UpgradeCheckResult {
    let mut results = Vec::new();

    // Check that there is only one Vcs field
    let Ok(control) = ws.parsed_control() else {
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
    if let Err(reason) = check_copyright_files_not_directories(ws, base_path) {
        return UpgradeCheckResult::Failure {
            section: "copyright-format".to_string(),
            reason,
        };
    }

    results.push("Files entries in debian/copyright don't refer to directories".to_string());
    UpgradeCheckResult::Success(results)
}

fn check_changelog_epoch_changes(ws: &dyn FixerWorkspace) -> bool {
    let Ok(cl) = ws.parsed_changelog() else {
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

fn check_4_1_5(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    if check_changelog_epoch_changes(ws) {
        return UpgradeCheckResult::Unable {
            section: "5.6.12".to_string(),
            reason: "last release changes epoch".to_string(),
        };
    }

    UpgradeCheckResult::Success(vec!["Package did not recently introduce epoch".to_string()])
}

fn poor_grep(ws: &dyn FixerWorkspace, rel: &Path, needle: &[u8]) -> bool {
    let Ok(Some(content)) = ws.read_file(rel) else {
        return false;
    };
    content.windows(needle.len()).any(|window| window == needle)
}

fn check_maintainer_scripts_for_users(ws: &dyn FixerWorkspace) -> Result<bool, UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(false);
    };

    let mut uses_update_rc_d = false;
    for name in entries {
        if !name.ends_with(".postinst") && !name.ends_with(".preinst") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&name);
        if poor_grep(ws, &rel, b"adduser") || poor_grep(ws, &rel, b"useradd") {
            return Err(UpgradeCheckResult::Unable {
                section: "9.2.1".to_string(),
                reason: "dynamically generated usernames should start with an underscore"
                    .to_string(),
            });
        }
        if poor_grep(ws, &rel, b"update-rc.d") {
            uses_update_rc_d = true;
        }
    }

    Ok(uses_update_rc_d)
}

fn check_init_files_have_systemd_units(
    ws: &dyn FixerWorkspace,
    uses_update_rc_d: bool,
) -> Result<(), UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };
    let entries_set: std::collections::HashSet<&String> = entries.iter().collect();

    for name in &entries {
        if !name.ends_with(".init") {
            continue;
        }
        let shortname = &name[..name.len() - 5];
        let service = format!("{}.service", shortname);
        let template_service = format!("{}@.service", shortname);
        if !entries_set.contains(&service) && !entries_set.contains(&template_service) {
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

fn check_4_5_0(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    if !matches!(ws.list_dir(Path::new("debian")), Ok(Some(_))) {
        return UpgradeCheckResult::Success(vec![
            "Package does not create users".to_string(),
            "Package does not ship init files".to_string(),
        ]);
    }

    let uses_update_rc_d = match check_maintainer_scripts_for_users(ws) {
        Ok(uses) => uses,
        Err(result) => return result,
    };

    let mut results = vec!["Package does not create users".to_string()];

    if let Err(result) = check_init_files_have_systemd_units(ws, uses_update_rc_d) {
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

fn check_4_5_1(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian/patches")) else {
        return UpgradeCheckResult::Success(vec!["Package does not have any patches".to_string()]);
    };

    if entries.iter().any(|n| n.ends_with(".series")) {
        return UpgradeCheckResult::Failure {
            section: "4.5.1".to_string(),
            reason: "package contains non-default series file".to_string(),
        };
    }

    UpgradeCheckResult::Success(vec![
        "Package does not ship any non-default series files".to_string()
    ])
}

fn check_4_2_1(_ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    UpgradeCheckResult::Success(vec![])
}

fn check_for_lib64_references(ws: &dyn FixerWorkspace) -> Result<(), UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };
    for name in entries {
        let rel = PathBuf::from("debian").join(&name);
        if poor_grep(ws, &rel, b"lib64") {
            return Err(UpgradeCheckResult::Unable {
                section: "9.1.1".to_string(),
                reason: "unable to verify whether package install files into /usr/lib/64"
                    .to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_6_0(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    if !matches!(ws.list_dir(Path::new("debian")), Ok(Some(_))) {
        return UpgradeCheckResult::Success(vec![
            "Package does not contain any references to lib64".to_string(),
        ]);
    }

    if let Err(result) = check_for_lib64_references(ws) {
        return result;
    }

    UpgradeCheckResult::Success(vec![
        "Package does not contain any references to lib64".to_string()
    ])
}

fn check_4_6_1(_ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 9.1.1: Restore permission for packages for non-64-bit architectures to
    // install files to /usr/lib64/.
    // -> No need to check anything.
    UpgradeCheckResult::Success(vec![])
}

fn check_for_x_window_manager(ws: &dyn FixerWorkspace) -> Result<(), UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };
    for name in entries {
        let rel = PathBuf::from("debian").join(&name);
        if poor_grep(ws, &rel, b"x-window-manager") {
            return Err(UpgradeCheckResult::Unable {
                section: "11.8.4".to_string(),
                reason: "unable to verify priority for /usr/bin/x-window-manager alternative"
                    .to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_6_2(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    if !matches!(ws.list_dir(Path::new("debian")), Ok(Some(_))) {
        return UpgradeCheckResult::Success(vec![
            "Package does not provide x-window-manager alternative".to_string(),
        ]);
    }

    if let Err(result) = check_for_x_window_manager(ws) {
        return result;
    }

    UpgradeCheckResult::Success(vec![
        "Package does not provide x-window-manager alternative".to_string(),
    ])
}

fn check_for_dpkg_divert(ws: &dyn FixerWorkspace) -> Result<(), UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };
    for name in entries {
        if !name.ends_with(".postinst")
            && !name.ends_with(".preinst")
            && !name.ends_with(".postrm")
            && !name.ends_with(".prerm")
        {
            continue;
        }
        let rel = PathBuf::from("debian").join(&name);
        if poor_grep(ws, &rel, b"dpkg-divert") {
            return Err(UpgradeCheckResult::Unable {
                section: "3.9".to_string(),
                reason: "unable to verify dpkg-divert usage follows new policy requirements"
                    .to_string(),
            });
        }
    }

    Ok(())
}

fn check_4_7_0(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 3.9: maintainer scripts should prefer native mechanisms over dpkg-divert;
    // must not divert systemd configuration files
    if !matches!(ws.list_dir(Path::new("debian")), Ok(Some(_))) {
        return UpgradeCheckResult::Success(vec!["Package does not use dpkg-divert".to_string()]);
    }

    if let Err(result) = check_for_dpkg_divert(ws) {
        return result;
    }

    UpgradeCheckResult::Success(vec!["Package does not use dpkg-divert".to_string()])
}

fn check_for_non_usr_paths(ws: &dyn FixerWorkspace) -> Result<(), UpgradeCheckResult> {
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };

    for name in entries {
        if !name.ends_with(".install") && !name.ends_with(".dirs") {
            continue;
        }
        let rel = PathBuf::from("debian").join(&name);
        let Ok(Some(bytes)) = ws.read_file(&rel) else {
            continue;
        };
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Check destination (last path component in .install files)
            let dest = if name.ends_with(".install") {
                line.split_whitespace().last().unwrap_or(line)
            } else {
                line
            };
            if dest == "/bin"
                || dest.starts_with("/bin/")
                || dest == "/sbin"
                || dest.starts_with("/sbin/")
                || dest == "/lib"
                || dest.starts_with("/lib/")
                || dest.starts_with("/lib32")
                || dest.starts_with("/lib64")
            {
                return Err(UpgradeCheckResult::Unable {
                    section: "10.1".to_string(),
                    reason: "package installs files to non-/usr paths".to_string(),
                });
            }
        }
    }

    Ok(())
}

fn check_4_7_1(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 10.1: packages must not install files to /bin, /lib, /lib*, /sbin
    if !matches!(ws.list_dir(Path::new("debian")), Ok(Some(_))) {
        return UpgradeCheckResult::Success(vec![
            "Package does not install to non-/usr paths".to_string()
        ]);
    }

    if let Err(result) = check_for_non_usr_paths(ws) {
        return result;
    }

    UpgradeCheckResult::Success(vec![
        "Package does not install to non-/usr paths".to_string()
    ])
}

fn check_4_7_2(_ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 10.1: Relaxation of previous restrictions for /usr/games.
    // No new requirements to check.
    UpgradeCheckResult::Success(vec![])
}

fn check_4_7_3(_ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 5.6.6: Priority field no longer recommended; dpkg defaults to optional.
    // 5.6.32 & 5.6.33: New documentation for Git-Tag-Tagger and Git-Tag-Info fields.
    // No new requirements to check.
    UpgradeCheckResult::Success(vec![])
}

fn check_4_7_4(ws: &dyn FixerWorkspace, _base_path: &Path) -> UpgradeCheckResult {
    // 8.4: *.so files in shared library development packages may be linker scripts
    //      instead of symbolic links. (Relaxation, no check needed.)
    // 12.5: The requirement to explain in the copyright file why the package is not
    //       part of the Debian distribution also applies to packages in non-free-firmware.
    let Ok(control) = ws.parsed_control() else {
        return UpgradeCheckResult::Success(
            vec!["Package is not in non-free-firmware".to_string()],
        );
    };
    let section = control.source().and_then(|s| s.section());
    if section.as_deref() == Some("non-free-firmware")
        || section
            .as_deref()
            .is_some_and(|s| s.starts_with("non-free-firmware/"))
    {
        return UpgradeCheckResult::Unable {
            section: "12.5".to_string(),
            reason: "unable to verify copyright file explains why package is in non-free-firmware"
                .to_string(),
        };
    }

    UpgradeCheckResult::Success(vec!["Package is not in non-free-firmware".to_string()])
}

fn get_check_fn(version: &str) -> Option<fn(&dyn FixerWorkspace, &Path) -> UpgradeCheckResult> {
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
        "4.7.0" => Some(check_4_7_0),
        "4.7.1" => Some(check_4_7_1),
        "4.7.2" => Some(check_4_7_2),
        "4.7.3" => Some(check_4_7_3),
        "4.7.4" => Some(check_4_7_4),
        _ => None,
    }
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // Debcargo packages manage their own control file — skip.
    if ws.read_file(Path::new("debian/debcargo.toml"))?.is_some() {
        return Ok(Vec::new());
    }

    // check_copyright_files_not_directories does is_dir() probes against
    // arbitrary glob targets — only resolvable on a real on-disk base.
    // LSP hosts won't supply one and skip the standards-version bump.
    let Some(base_path) = ws.base_path() else {
        return Ok(Vec::new());
    };
    let base_path = base_path.to_path_buf();

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    let current_version_str = match source.standards_version() {
        Some(sv) => {
            tracing::debug!("Current standards version: {}", sv);
            sv
        }
        None => {
            tracing::debug!("No standards version found");
            return Ok(Vec::new());
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
                return Ok(Vec::new());
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
            Err(_) => return Ok(Vec::new()),
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

    // Now try to upgrade through the path
    let mut current = current_version_str.clone();
    let path = upgrade_path();
    let mut upgrade_reasons: Vec<(String, String, Vec<String>)> = Vec::new();

    while let Some(&target) = path.get(current.as_str()) {
        if let Some(check_fn) = get_check_fn(target) {
            match check_fn(ws, &base_path) {
                UpgradeCheckResult::Success(reasons) => {
                    if !reasons.is_empty() {
                        upgrade_reasons.push((current.clone(), target.to_string(), reasons));
                    }
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
        return Ok(Vec::new());
    }

    let mut label = format!(
        "Update standards version to {}, no changes needed.",
        current
    );

    if !upgrade_reasons.is_empty() {
        label.push_str("\n\nUpgrade checklist verified:");
        for (from, to, reasons) in &upgrade_reasons {
            label.push_str(&format!("\n {} → {}:", from, to));
            for reason in reasons {
                label.push_str(&format!("\n  * {}", reason));
            }
        }
    }

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Standards-Version is out of date.",
        label,
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Standards-Version".into(),
            value: current,
        })],
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "out-of-date-standards-version",
    tags: ["out-of-date-standards-version", "ancient-standards-version"],
    // Standards version should only be bumped after all other fixes are applied
    after: [
        "file-contains-trailing-whitespace",
        "out-of-date-copyright-format-uri",
        "missing-vcs-browser-field"
    ],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Standards-Version",
        },
        // Upgrade-checklist callbacks read these too.
        crate::workspace::Trigger::Changelog(crate::workspace::ChangelogAspect::Version),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "Files",
        },
        crate::workspace::Trigger::File("debian/compat"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}
