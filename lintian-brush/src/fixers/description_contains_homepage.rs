use crate::declare_detector;
use crate::diagnostic::{
    Action, ActionPlan, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector,
};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const DESCRIPTION: &str = "Description contains a Homepage pseudo-field.";

/// Whether a continuation line is a bare paragraph separator.
///
/// deb822 renders `^ \.\s*$` in the raw file as `.` optionally followed
/// by whitespace once the leading indent is stripped.
fn is_bare_separator(line: &str) -> bool {
    line.strip_prefix('.')
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Match a description line that lintian would flag as a Homepage
/// pseudo-field. Returns the extracted URL (with any surrounding
/// `<...>` stripped) if the line matches, else `None`.
///
/// Lintian's regex is `^\s*Homepage: <?https?://` applied
/// case-insensitively. We match the same shape and, on success, pull
/// out the URL so the fixer can move it to the Source paragraph.
fn parse_pseudo_homepage(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .get(..8)
        .filter(|prefix| prefix.eq_ignore_ascii_case("homepage"))
        .and(trimmed.get(8..))?;
    let after_colon = rest.strip_prefix(':')?.trim_start();
    let (url, _rest) = if let Some(inside) = after_colon.strip_prefix('<') {
        let end = inside.find('>')?;
        (&inside[..end], &inside[end + 1..])
    } else {
        let end = after_colon
            .find(char::is_whitespace)
            .unwrap_or(after_colon.len());
        (&after_colon[..end], &after_colon[end..])
    };
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Result of scanning an extended description for a Homepage pseudo-field.
#[derive(Debug, PartialEq, Eq)]
struct HomepageMatch {
    /// 1-indexed line number within the extended description (matches
    /// the value lintian puts in the tag info).
    line: usize,
    /// 0-indexed position of the extended-description line in the
    /// original `\n`-split value, so the fix can drop it.
    idx: usize,
    /// The URL extracted from the pseudo-field.
    url: String,
}

/// Find the first `Homepage:` pseudo-field in the extended description,
/// matching lintian's behaviour. The synopsis (first line) is ignored.
///
/// Bare paragraph separators count towards the line number, matching
/// how lintian numbers lines in the raw file.
fn find_pseudo_homepage(description: &str) -> Option<HomepageMatch> {
    let mut lines = description.split('\n');
    lines.next();
    for (idx, line) in lines.enumerate() {
        if is_bare_separator(line) {
            continue;
        }
        if let Some(url) = parse_pseudo_homepage(line) {
            return Some(HomepageMatch {
                line: idx + 1,
                idx,
                url,
            });
        }
    }
    None
}

/// Remove line `idx` from the extended description and collapse
/// adjacent blank/separator lines it may have left dangling.
fn drop_extended_line(description: &str, idx: usize) -> String {
    let mut parts: Vec<&str> = description.split('\n').collect();
    // idx is 0-indexed within the extended description, i.e. skipping
    // the synopsis at parts[0].
    let target = idx + 1;
    if target >= parts.len() {
        return description.to_string();
    }
    parts.remove(target);

    // If removing the line leaves two adjacent bare separators (or a
    // separator right after the synopsis), collapse one of them.
    if target < parts.len()
        && target >= 1
        && is_bare_separator(parts[target])
        && (target == 1 || is_bare_separator(parts[target - 1]))
    {
        parts.remove(target);
    }
    // Drop a trailing bare separator that was left dangling at the end.
    while parts.len() > 1 && is_bare_separator(parts[parts.len() - 1]) {
        parts.pop();
    }
    parts.join("\n")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let existing_homepage = control
        .source()
        .and_then(|s| s.as_deb822().get("Homepage"))
        .map(|s| s.trim().to_string());

    let mut diagnostics = Vec::new();
    let mut chosen_url: Option<String> = existing_homepage.clone();

    for binary in control.binaries() {
        let Some(description) = binary.description() else {
            continue;
        };
        let Some(hit) = find_pseudo_homepage(&description) else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-contains-homepage",
            Visibility::Warning,
            vec![format!("line {}", hit.line)],
        );

        // If the source already declares a Homepage that differs from
        // the URL we're extracting, we don't want to silently overwrite
        // it. Emit the diagnostic without a fix in that case.
        let can_promote = match &existing_homepage {
            Some(existing) => existing == &hit.url,
            None => true,
        };

        let new_description = drop_extended_line(&description, hit.idx);
        let mut actions: Vec<Action> = Vec::new();

        if can_promote {
            // Only add a Homepage action for the first binary whose URL
            // we're promoting; subsequent binaries just get their pseudo
            // field stripped. We track the URL so mismatched siblings
            // are also left alone.
            let want_url = chosen_url.get_or_insert(hit.url.clone()).clone();
            if want_url != hit.url {
                // Another binary already claimed a different URL for
                // this run; don't promote a conflicting one.
                diagnostics.push(Diagnostic::with_plans(issue, DESCRIPTION, Vec::new()));
                continue;
            }
            if existing_homepage.as_deref() != Some(want_url.as_str()) {
                actions.push(Action::Deb822(Deb822Action::SetField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: "Homepage".into(),
                    value: want_url,
                }));
            }
            actions.push(Action::Deb822(Deb822Action::SetFieldWithIndent {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary {
                    package: package_name.clone(),
                },
                field: "Description".into(),
                value: new_description,
                indent: IndentPattern::Fixed { spaces: 1 },
            }));
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    DESCRIPTION,
                    format!(
                        "Move Homepage pseudo-field from description of {} to Source paragraph.",
                        package_name
                    ),
                    actions,
                )
                .with_certainty(Certainty::Certain),
            );
        } else {
            // Existing Homepage disagrees with the pseudo-field URL:
            // record the issue but don't apply a fix.
            diagnostics.push(Diagnostic::with_plans(issue, DESCRIPTION, Vec::new()));
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let mut packages: Vec<String> = fixed
        .iter()
        .filter_map(|(d, _)| d.issue.as_ref()?.package.clone())
        .collect();
    packages.sort();
    packages.dedup();
    match packages.as_slice() {
        [] => "Move Homepage pseudo-field from description to Source paragraph.".to_string(),
        [pkg] => format!(
            "Move Homepage pseudo-field from description of {} to Source paragraph.",
            pkg
        ),
        _ => format!(
            "Move Homepage pseudo-field from descriptions of {} to Source paragraph.",
            packages.join(", ")
        ),
    }
}

declare_detector! {
    name: "description-contains-homepage",
    tags: ["description-contains-homepage"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, PackageType, Version};
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_parse_pseudo_homepage_plain() {
        assert_eq!(
            parse_pseudo_homepage("Homepage: https://example.com/"),
            Some("https://example.com/".to_string())
        );
    }

    #[test]
    fn test_parse_pseudo_homepage_angle_brackets() {
        assert_eq!(
            parse_pseudo_homepage("Homepage: <https://example.com/>"),
            Some("https://example.com/".to_string())
        );
    }

    #[test]
    fn test_parse_pseudo_homepage_case_insensitive() {
        assert_eq!(
            parse_pseudo_homepage("HOMEPAGE: http://example.com"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            parse_pseudo_homepage("homepage: HTTP://EXAMPLE.COM"),
            Some("HTTP://EXAMPLE.COM".to_string())
        );
    }

    #[test]
    fn test_parse_pseudo_homepage_with_leading_ws() {
        assert_eq!(
            parse_pseudo_homepage("   Homepage: https://example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn test_parse_pseudo_homepage_rejects_non_url() {
        assert_eq!(parse_pseudo_homepage("Homepage: not a url"), None);
        assert_eq!(parse_pseudo_homepage("Homepage: ftp://example.com"), None);
        assert_eq!(parse_pseudo_homepage("Not a homepage line"), None);
        assert_eq!(parse_pseudo_homepage("Homepage https://example.com"), None);
    }

    #[test]
    fn test_find_pseudo_homepage_extended() {
        // Description values come from deb822 with one leading space
        // stripped from each continuation line.
        let desc = "Synopsis line\nSome text.\n.\nHomepage: https://foo/";
        let hit = find_pseudo_homepage(desc).unwrap();
        assert_eq!(
            hit,
            HomepageMatch {
                line: 3,
                idx: 2,
                url: "https://foo/".to_string(),
            }
        );
    }

    #[test]
    fn test_find_pseudo_homepage_skips_synopsis() {
        // A homepage-shaped synopsis is not flagged: lintian only
        // scans the extended description.
        let desc = "Homepage: https://foo/\nActual description.";
        assert_eq!(find_pseudo_homepage(desc), None);
    }

    #[test]
    fn test_find_pseudo_homepage_counts_separators() {
        // Bare separators are skipped but still count towards the
        // line number lintian reports.
        let desc = "Synopsis\nFirst para.\n.\nHomepage: https://foo/";
        let hit = find_pseudo_homepage(desc).unwrap();
        assert_eq!(hit.line, 3);
    }

    #[test]
    fn test_drop_extended_line_middle() {
        // Removing a middle line collapses the surrounding separators
        // so we don't leave two blank paragraphs stacked together.
        let desc = "Syn\nFirst.\n.\nHomepage: https://foo/\n.\nLast.";
        let cleaned = drop_extended_line(desc, 2);
        assert_eq!(cleaned, "Syn\nFirst.\n.\nLast.");
    }

    #[test]
    fn test_drop_extended_line_trailing() {
        // A pseudo-field at the very end should not leave a dangling
        // separator behind.
        let desc = "Syn\nBody.\n.\nHomepage: https://foo/";
        let cleaned = drop_extended_line(desc, 2);
        assert_eq!(cleaned, "Syn\nBody.");
    }

    #[test]
    fn test_fix_promotes_url_to_source() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n Extended text.\n .\n Homepage: https://example.com/foo\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(
            result.description,
            "Move Homepage pseudo-field from description of foo to Source paragraph."
        );
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nHomepage: https://example.com/foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n Extended text.\n",
        );
    }

    #[test]
    fn test_fix_leaves_existing_matching_homepage() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nHomepage: https://example.com/foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n Extended.\n .\n Homepage: https://example.com/foo\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nHomepage: https://example.com/foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n Extended.\n",
        );
    }

    #[test]
    fn test_detect_when_existing_homepage_differs_no_fix() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nHomepage: https://example.com/other\n\nPackage: foo\nDescription: A tool\n Body.\n .\n Homepage: https://example.com/foo\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].plans.is_empty());
        let issue = diagnostics[0].issue.as_ref().unwrap();
        assert_eq!(issue.package.as_deref(), Some("foo"));
        assert_eq!(issue.package_type, Some(PackageType::Binary));
        assert_eq!(issue.info.as_deref(), Some("line 3"));

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_pseudo_homepage() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: A tool\n Body only.\n",
        );

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_binaries_same_url() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo1\nDescription: First\n Body1.\n .\n Homepage: https://example.com/foo\n\nPackage: foo2\nDescription: Second\n Body2.\n .\n Homepage: https://example.com/foo\n",
        );

        run_apply(base).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nHomepage: https://example.com/foo\n\nPackage: foo1\nDescription: First\n Body1.\n\nPackage: foo2\nDescription: Second\n Body2.\n",
        );
    }

    #[test]
    fn test_multiple_binaries_conflicting_urls_only_first_promoted() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo1\nDescription: First\n Body1.\n .\n Homepage: https://example.com/foo1\n\nPackage: foo2\nDescription: Second\n Body2.\n .\n Homepage: https://example.com/foo2\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 2);
        // The first binary carries a fix, the second only reports.
        assert!(!diagnostics[0].plans.is_empty());
        assert!(diagnostics[1].plans.is_empty());
    }
}
