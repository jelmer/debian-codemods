use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MaintscriptAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences};
use chrono::{DateTime, NaiveDate, Utc};
use debian_analyzer::maintscripts::{Entry, Maintscript};
use debian_changelog::ChangeLog;
use debversion::Version;
use distro_info::{DebianDistroInfo, DistroInfo};
use std::path::{Path, PathBuf};
use std::str::FromStr;

// If there is no information from the upgrade release, default to 5 years.
const DEFAULT_AGE_THRESHOLD_DAYS: i64 = 5 * 365;

fn find_maintscript_files(ws: &dyn FixerWorkspace) -> Result<Vec<String>, FixerError> {
    let mut entries = match ws.list_dir(Path::new("debian"))? {
        Some(e) => e,
        None => return Ok(vec![]),
    };
    entries.sort();
    Ok(entries
        .into_iter()
        .filter(|name| name == "maintscript" || name.ends_with(".maintscript"))
        .collect())
}

fn get_date_threshold(upgrade_release: Option<&str>) -> Result<NaiveDate, FixerError> {
    // Try to get the release date from distro-info
    if let Some(release) = upgrade_release {
        if let Ok(debian_info) = DebianDistroInfo::new() {
            // Find the release by codename or series
            let all_releases = debian_info.all_at(Utc::now().naive_utc().date());

            for series in all_releases {
                if series.codename().eq_ignore_ascii_case(release) || series.series() == release {
                    if let Some(release_date) = series.release() {
                        return Ok(*release_date);
                    }
                }
            }
        }
    }

    // Default to 5 years ago
    let now = Utc::now();
    let threshold = now.date_naive() - chrono::Duration::days(DEFAULT_AGE_THRESHOLD_DAYS);
    Ok(threshold)
}

fn parse_changelog_dates(
    ws: &dyn FixerWorkspace,
) -> Result<Vec<(Version, DateTime<Utc>)>, FixerError> {
    let bytes = match ws.read_file(Path::new("debian/changelog"))? {
        Some(b) => b,
        None => return Ok(vec![]),
    };
    let changelog = ChangeLog::read_relaxed(bytes.as_slice())
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {:?}", e)))?;

    let mut dates = Vec::new();

    for entry in changelog.iter() {
        if let Some(version) = entry.version() {
            match entry.datetime() {
                Some(dt) => {
                    // datetime() already returns a parsed DateTime<FixedOffset>
                    dates.push((version.clone(), dt.with_timezone(&Utc)));
                }
                None => {
                    // If we can't parse a date, we can't reliably check anymore
                    // This matches the Python behavior
                    if let Some(timestamp) = entry.timestamp() {
                        return Err(FixerError::Other(format!(
                            "Invalid date {:?} for {}",
                            timestamp, version
                        )));
                    }
                }
            }
        }
    }

    Ok(dates)
}

fn is_well_past(
    version: &Version,
    cl_dates: &[(Version, DateTime<Utc>)],
    date_threshold: &NaiveDate,
) -> bool {
    // Check if ALL changelog entries for this version or later were before the threshold
    for (cl_version, cl_dt) in cl_dates {
        if cl_version <= version && cl_dt.date_naive() > *date_threshold {
            return false;
        }
    }
    true
}

/// Information about a maintscript entry that should be removed.
struct RemovedEntry {
    entry: Entry,
}

/// Parse a maintscript file and return the entries that should be removed.
fn obsolete_maintscript_entries<F>(
    ws: &dyn FixerWorkspace,
    rel: &Path,
    should_remove: F,
) -> Result<Vec<RemovedEntry>, FixerError>
where
    F: Fn(Option<&str>, &Version) -> bool,
{
    let bytes = match ws.read_file(rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let contents = String::from_utf8(bytes)
        .map_err(|e| FixerError::Other(format!("Failed to read maintscript: {}", e)))?;
    let script = Maintscript::from_str(&contents)
        .map_err(|e| FixerError::Other(format!("Failed to parse maintscript: {}", e)))?;
    let mut removed = Vec::new();
    for entry in script.entries() {
        let should = entry
            .prior_version()
            .map(|v| should_remove(entry.package().map(|s| s.as_str()), v))
            .unwrap_or(false);
        if should {
            removed.push(RemovedEntry {
                entry: entry.clone(),
            });
        }
    }
    Ok(removed)
}

/// Find the upload date for a given version in the changelog dates.
fn find_version_date(
    version: &Version,
    cl_dates: &[(Version, DateTime<Utc>)],
) -> Option<DateTime<Utc>> {
    cl_dates
        .iter()
        .find(|(v, _)| v == version)
        .map(|(_, dt)| *dt)
}

/// Format a detail line for a removed maintscript entry, including the version
/// it applied to and when that version was uploaded.
fn format_removed_entry_detail(
    entry: &RemovedEntry,
    cl_dates: &[(Version, DateTime<Utc>)],
) -> String {
    let version = entry.entry.prior_version().unwrap();
    let date_info = find_version_date(version, cl_dates)
        .map(|dt| format!(", uploaded on {}", dt.format("%Y-%m-%d")))
        .unwrap_or_default();
    format!("\"{}\" (version {}{})", entry.entry, version, date_info)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let maintscripts = find_maintscript_files(ws)?;
    if maintscripts.is_empty() {
        return Ok(Vec::new());
    }

    let date_threshold = get_date_threshold(preferences.upgrade_release.as_deref())?;
    let cl_dates = parse_changelog_dates(ws)?;

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for name in maintscripts {
        let rel = PathBuf::from("debian").join(&name);
        let removed = obsolete_maintscript_entries(ws, &rel, |_package, version| {
            is_well_past(version, &cl_dates, &date_threshold)
        })?;
        for r in removed {
            let detail = format_removed_entry_detail(&r, &cl_dates);
            diagnostics.push(Diagnostic::untagged(
                detail,
                vec![Action::Maintscript(MaintscriptAction::DropEntry {
                    file: rel.clone(),
                    entry: r.entry.to_string(),
                })],
            ));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let summary = if fixed.len() == 1 {
        "Remove an obsolete maintscript entry.".to_string()
    } else {
        format!("Remove {} obsolete maintscript entries.", fixed.len())
    };
    let details: Vec<&str> = fixed.iter().map(|d| d.message.as_str()).collect();
    format!("{}\n\n{}", summary, details.join("\n"))
}

declare_detector! {
    name: "ancient-maintscript-entry",
    tags: [],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_well_past() {
        use chrono::TimeZone;

        let version = Version::from_str("0.1-1").unwrap();
        let cl_dates = vec![
            (
                Version::from_str("0.1-2").unwrap(),
                Utc.with_ymd_and_hms(2011, 3, 22, 16, 47, 42).unwrap(),
            ),
            (
                Version::from_str("0.1-1").unwrap(),
                Utc.with_ymd_and_hms(2011, 3, 22, 16, 47, 31).unwrap(),
            ),
        ];
        let threshold = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();

        assert!(is_well_past(&version, &cl_dates, &threshold));
    }

    #[test]
    fn test_not_well_past() {
        use chrono::TimeZone;

        let version = Version::from_str("0.1-1").unwrap();
        let cl_dates = vec![
            (
                Version::from_str("0.1-2").unwrap(),
                Utc.with_ymd_and_hms(2021, 3, 22, 16, 47, 42).unwrap(),
            ),
            (
                Version::from_str("0.1-1").unwrap(),
                Utc.with_ymd_and_hms(2021, 3, 22, 16, 47, 31).unwrap(),
            ),
        ];
        let threshold = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();

        assert!(!is_well_past(&version, &cl_dates, &threshold));
    }
}
