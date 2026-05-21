use crate::declare_detector;
use crate::diagnostic::{Action, Dep3Action, Diagnostic};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use chrono::NaiveDate;
use debian_workspace::Workspace;
use patchkit::quilt::SeriesEntry;
use std::path::PathBuf;

/// Mirror lintian's `check_wrong_rfc3339_date`: a `Last-Update` value is
/// valid only when it is a literal `YYYY-MM-DD` string denoting a real
/// calendar date with a year >= 1900.
fn is_wrong_last_update(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        return true;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        parts[0].parse::<i32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) else {
        return true;
    };
    year < 1900 || NaiveDate::from_ymd_opt(year, month, day).is_none()
}

/// Try to recover the date a non-ISO `Last-Update` value denotes.
///
/// Only formats whose field order is unambiguous are accepted, so the
/// reformat never has to guess: a four-digit leading year fixes the
/// `Y-M-D` order for numeric input, and a textual month name fixes it
/// for the remaining cases.
fn normalize_date(raw: &str) -> Option<NaiveDate> {
    // Year-first numeric (`YYYY<sep>M<sep>D`), optionally followed by a
    // time component after a space or `T` (e.g. full RFC 3339 timestamps).
    let date_part = raw.split([' ', 'T']).next().unwrap_or(raw);
    for sep in ['-', '/', '.'] {
        let comps: Vec<&str> = date_part.split(sep).collect();
        if comps.len() != 3 {
            continue;
        }
        if let (Ok(year), Ok(month), Ok(day)) = (
            comps[0].parse::<i32>(),
            comps[1].parse::<u32>(),
            comps[2].parse::<u32>(),
        ) {
            if (1900..=9999).contains(&year) {
                if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                    return Some(date);
                }
            }
        }
    }

    // RFC 2822 timestamps, e.g. as produced by `git format-patch`
    // ("Tue, 1 Jul 2003 10:52:37 +0200"). The date is taken in the
    // written offset rather than converted to UTC.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(raw) {
        return Some(dt.date_naive());
    }

    // A date carrying a textual (English) month and no time component.
    for fmt in [
        "%d %b %Y",
        "%d %B %Y",
        "%b %d, %Y",
        "%B %d, %Y",
        "%b %d %Y",
        "%B %d %Y",
    ] {
        if let Ok(date) = NaiveDate::parse_from_str(raw, fmt) {
            return Some(date);
        }
    }

    None
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let Some(series) = ws.parsed_patches_series()? else {
        return Ok(Vec::new());
    };
    let patches_rel = PathBuf::from("debian/patches");

    let mut diagnostics = Vec::new();
    for entry in &series.entries {
        let SeriesEntry::Patch { name, .. } = entry else {
            continue;
        };
        let patch_rel = patches_rel.join(name);
        let Some((Some(header), _)) = ws.parsed_patch(&patch_rel)? else {
            continue;
        };
        let Some(raw) = header.as_deb822().get("Last-Update") else {
            continue;
        };
        let raw = raw.trim();
        if !is_wrong_last_update(raw) {
            continue;
        }
        let Some(date) = normalize_date(raw) else {
            continue;
        };
        let fixed = date.format("%Y-%m-%d").to_string();
        // Only act when the reformatted value actually clears the tag.
        if is_wrong_last_update(&fixed) {
            continue;
        }

        let issue = LintianIssue::source_with_info(
            "invalid-dep3-format-patch-wrong-last-update",
            Visibility::Info,
            vec![format!("[debian/patches/{}]", name)],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "Last-Update field in DEP-3 patch header uses a non-ISO date format.",
                "Reformat the Last-Update date as ISO YYYY-MM-DD.",
                vec![Action::Dep3(Dep3Action::SetField {
                    file: patch_rel,
                    field: "Last-Update".into(),
                    value: fixed,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "invalid-dep3-format-patch-wrong-last-update",
    tags: ["invalid-dep3-format-patch-wrong-last-update"],
    triggers: [
        debian_workspace::Trigger::File("debian/patches/series"),
        debian_workspace::Trigger::Glob("debian/patches/*"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_patch(base: &Path, last_update: &str) -> PathBuf {
        let patches = base.join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix.patch\n").unwrap();
        let patch = patches.join("fix.patch");
        fs::write(
            &patch,
            format!(
                "Description: Fix a typo\nLast-Update: {}\n\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-teh\n+the\n",
                last_update
            ),
        )
        .unwrap();
        patch
    }

    #[test]
    fn test_is_wrong_last_update() {
        assert!(!is_wrong_last_update("2020-01-15"));
        assert!(!is_wrong_last_update("1999-12-31"));
        assert!(is_wrong_last_update("2020-1-5"));
        assert!(is_wrong_last_update("2020/01/15"));
        assert!(is_wrong_last_update("2020-02-30"));
        assert!(is_wrong_last_update("1899-01-01"));
        assert!(is_wrong_last_update("2020-13-01"));
        assert!(is_wrong_last_update(""));
    }

    #[test]
    fn test_normalize_date() {
        let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(normalize_date("2020/01/15"), Some(ymd(2020, 1, 15)));
        assert_eq!(normalize_date("2020.01.15"), Some(ymd(2020, 1, 15)));
        assert_eq!(normalize_date("2020-1-5"), Some(ymd(2020, 1, 5)));
        assert_eq!(
            normalize_date("2011-03-29 12:00:00"),
            Some(ymd(2011, 3, 29))
        );
        assert_eq!(
            normalize_date("2011-03-29T12:00:00Z"),
            Some(ymd(2011, 3, 29))
        );
        assert_eq!(
            normalize_date("Tue, 1 Jul 2003 10:52:37 +0200"),
            Some(ymd(2003, 7, 1))
        );
        assert_eq!(normalize_date("29 Mar 2010"), Some(ymd(2010, 3, 29)));
        assert_eq!(normalize_date("March 29, 2010"), Some(ymd(2010, 3, 29)));
        // Ambiguous day-first / month-first input is left alone.
        assert_eq!(normalize_date("15/01/2020"), None);
        assert_eq!(normalize_date("01-07-2003"), None);
        assert_eq!(normalize_date("not a date"), None);
    }

    #[test]
    fn test_reformats_slash_separated_date() {
        let tmp = TempDir::new().unwrap();
        let patch = write_patch(tmp.path(), "2020/01/15");
        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&patch).unwrap();
        assert!(updated.contains("Last-Update: 2020-01-15"));
        assert!(updated.contains("+the"));
    }

    #[test]
    fn test_reformats_timestamp() {
        let tmp = TempDir::new().unwrap();
        let patch = write_patch(tmp.path(), "2011-03-29 12:00:00");
        run_apply(tmp.path()).unwrap();
        let updated = fs::read_to_string(&patch).unwrap();
        assert!(updated.contains("Last-Update: 2011-03-29"));
    }

    #[test]
    fn test_no_changes_when_already_valid() {
        let tmp = TempDir::new().unwrap();
        write_patch(tmp.path(), "2020-01-15");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_ambiguous() {
        let tmp = TempDir::new().unwrap();
        write_patch(tmp.path(), "15/01/2020");
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_last_update() {
        let tmp = TempDir::new().unwrap();
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "fix.patch\n").unwrap();
        fs::write(
            patches.join("fix.patch"),
            "Description: Fix a typo\nAuthor: jane@example.com\n\n--- a/file.txt\n+++ b/file.txt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_series_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
