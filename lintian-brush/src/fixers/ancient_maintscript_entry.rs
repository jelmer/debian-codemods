use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use chrono::{DateTime, NaiveDate, Utc};
use debian_analyzer::maintscripts::{Entry, Maintscript};
use debian_changelog::ChangeLog;
use debversion::Version;
use distro_info::{DebianDistroInfo, DistroInfo};
use std::fs;
use std::path::Path;
use std::str::FromStr;

// If there is no information from the upgrade release, default to 5 years.
const DEFAULT_AGE_THRESHOLD_DAYS: i64 = 5 * 365;

fn find_maintscript_files(base_path: &Path) -> Result<Vec<String>, FixerError> {
    let debian_dir = base_path.join("debian");
    if !debian_dir.exists() {
        return Ok(vec![]);
    }

    let mut maintscripts = Vec::new();

    for entry in fs::read_dir(&debian_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str == "maintscript" || name_str.ends_with(".maintscript") {
            maintscripts.push(name_str.to_string());
        }
    }

    Ok(maintscripts)
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

fn parse_changelog_dates(base_path: &Path) -> Result<Vec<(Version, DateTime<Utc>)>, FixerError> {
    let changelog_path = base_path.join("debian/changelog");
    if !changelog_path.exists() {
        return Ok(vec![]);
    }

    let contents = fs::read_to_string(&changelog_path)?;
    let changelog = ChangeLog::read(&mut contents.as_bytes())
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

fn drop_obsolete_maintscript_entries<F>(
    maintscript_path: &Path,
    should_remove: F,
) -> Result<usize, FixerError>
where
    F: Fn(Option<&str>, &Version) -> bool,
{
    let contents = fs::read_to_string(maintscript_path)?;
    let lines: Vec<&str> = contents.lines().collect();

    let mut new_lines = Vec::new();
    let mut comments = Vec::new();
    let mut removed_count = 0;

    for line in lines {
        let trimmed = line.trim();

        // Accumulate comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            comments.push(line);
            continue;
        }

        // Try to parse as entry
        match Entry::from_str(trimmed) {
            Ok(entry) => {
                // Check if this entry should be removed
                let remove = entry
                    .prior_version()
                    .map(|v| should_remove(entry.package().map(|s| s.as_str()), v))
                    .unwrap_or(false);

                if remove {
                    comments.clear();
                    removed_count += 1;
                } else {
                    new_lines.extend(comments.drain(..).map(|s| s.to_string()));
                    new_lines.push(line.to_string());
                }
            }
            Err(_) => {
                // Not a parseable entry, keep it
                new_lines.extend(comments.drain(..).map(|s| s.to_string()));
                new_lines.push(line.to_string());
            }
        }
    }

    // Add trailing comments
    new_lines.extend(comments.into_iter().map(|s| s.to_string()));

    if removed_count == 0 {
        return Ok(0);
    }

    // Delete file if empty, otherwise write
    if new_lines.is_empty() {
        fs::remove_file(maintscript_path)?;
    } else {
        let mut output = new_lines.join("\n");
        if contents.ends_with('\n') {
            output.push('\n');
        }
        fs::write(maintscript_path, output)?;
    }

    Ok(removed_count)
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let maintscripts = find_maintscript_files(base_path)?;

    if maintscripts.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Get the date threshold
    let date_threshold = get_date_threshold(preferences.upgrade_release.as_deref())?;

    // Parse changelog dates
    let cl_dates = parse_changelog_dates(base_path)?;

    // Process each maintscript file
    let mut total_entries = 0;
    let mut modified_files = 0;

    for name in maintscripts {
        let maintscript_path = base_path.join("debian").join(&name);

        let removed = drop_obsolete_maintscript_entries(&maintscript_path, |_package, version| {
            is_well_past(version, &cl_dates, &date_threshold)
        })?;

        if removed > 0 {
            total_entries += removed;
            modified_files += 1;
        }
    }

    if total_entries == 0 {
        return Err(FixerError::NoChanges);
    }

    let description = if total_entries == 1 {
        "Remove an obsolete maintscript entry.".to_string()
    } else {
        format!(
            "Remove {} obsolete maintscript entries in {} files.",
            total_entries, modified_files
        )
    };

    Ok(FixerResult::builder(description).build())
}

declare_fixer! {
    name: "ancient-maintscript-entry",
    tags: [],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
