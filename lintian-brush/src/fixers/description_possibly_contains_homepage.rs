use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;

const DESCRIPTION: &str = "Description possibly contains a homepage URL.";

/// Keywords lintian treats as hints that a URL in the extended
/// description is the project homepage. Matches
/// `homepage|webpage|website|url|upstream|web\s+site
///  |home\s+page|further\s+information|more\s+info
///  |official\s+site|project\s+home` case-insensitively.
fn has_homepage_keyword(extended: &str) -> bool {
    let lower = extended.to_ascii_lowercase();
    const SIMPLE: &[&str] = &["homepage", "webpage", "website", "url", "upstream"];
    if SIMPLE.iter().any(|kw| lower.contains(kw)) {
        return true;
    }
    // Phrases with internal whitespace: lintian uses `\s+`, so match any
    // run of whitespace between the two words.
    const PHRASES: &[(&str, &str)] = &[
        ("web", "site"),
        ("home", "page"),
        ("further", "information"),
        ("more", "info"),
        ("official", "site"),
        ("project", "home"),
    ];
    for (first, second) in PHRASES {
        if contains_whitespace_separated(&lower, first, second) {
            return true;
        }
    }
    false
}

fn contains_whitespace_separated(haystack: &str, first: &str, second: &str) -> bool {
    let mut rest = haystack;
    while let Some(pos) = rest.find(first) {
        let after = &rest[pos + first.len()..];
        let trimmed = after.trim_start();
        if trimmed.len() < after.len() && trimmed.starts_with(second) {
            return true;
        }
        rest = &rest[pos + first.len()..];
    }
    false
}

/// Lintian's URL regex: `\b(https?://[a-z0-9][^>\s]+)`, case-insensitive.
/// Returns the first URL match in `text`, or `None`.
fn find_first_url(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("http") {
        let start = search_from + rel;
        if !is_word_boundary_before(&lower, start) {
            search_from = start + 4;
            continue;
        }
        let after = &lower[start..];
        let scheme_len = if after.starts_with("https://") {
            8
        } else if after.starts_with("http://") {
            7
        } else {
            search_from = start + 4;
            continue;
        };
        let host_start = start + scheme_len;
        let host_char = lower.as_bytes().get(host_start).copied();
        if !host_char.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            search_from = host_start;
            continue;
        }
        // Walk forward over anything not whitespace and not `>`.
        let end = text[host_start..]
            .find(|c: char| c.is_whitespace() || c == '>')
            .map(|n| host_start + n)
            .unwrap_or(text.len());
        return Some(text[start..end].to_string());
    }
    None
}

fn is_word_boundary_before(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    text[..pos]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
}

/// Return the URL that lintian would flag for `extended`, or `None`
/// if the extended description would not trigger the tag.
///
/// Two paths trigger the tag:
///   1. A homepage-ish keyword is present, and a URL appears anywhere
///      in the extended description.
///   2. A URL appears at the very end of the extended description,
///      optionally followed by `>`, `.`, and trailing whitespace.
fn find_possible_homepage(extended: &str) -> Option<String> {
    if has_homepage_keyword(extended) {
        if let Some(url) = find_first_url(extended) {
            return Some(url);
        }
    }
    find_trailing_url(extended)
}

/// Match lintian's second branch:
/// `\b(https?://[a-z0-9][^>\s]+)>?\.?\s*\z` (case-insensitive).
///
/// The capture group is `[^>\s]+`, so the URL is terminated by whitespace
/// or `>`. The optional `>?\.?\s*\z` tail only constrains where in the
/// string the URL may sit: it must be the last non-whitespace token,
/// possibly followed by `>` then `.`.
fn find_trailing_url(text: &str) -> Option<String> {
    let text = text.trim_end();
    // Find the last URL in the text, honouring word-boundary and scheme.
    let scanned = find_last_scheme_url(text)?;
    // The remainder after the URL must consist only of an optional `>`
    // then an optional `.`.
    let tail = &text[scanned.end..];
    let tail = tail.strip_prefix('>').unwrap_or(tail);
    let tail = tail.strip_prefix('.').unwrap_or(tail);
    if !tail.is_empty() {
        return None;
    }
    Some(text[scanned.start..scanned.end].to_string())
}

struct UrlSpan {
    start: usize,
    end: usize,
}

/// Find the last `\b(https?://[a-z0-9][^>\s]+)` match in `text`, if any.
fn find_last_scheme_url(text: &str) -> Option<UrlSpan> {
    let lower = text.to_ascii_lowercase();
    let mut last: Option<UrlSpan> = None;
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("http") {
        let start = search_from + rel;
        search_from = start + 4;
        if !is_word_boundary_before(&lower, start) {
            continue;
        }
        let after = &lower[start..];
        let scheme_len = if after.starts_with("https://") {
            8
        } else if after.starts_with("http://") {
            7
        } else {
            continue;
        };
        let host_start = start + scheme_len;
        let host_char = lower.as_bytes().get(host_start).copied();
        if !host_char.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            continue;
        }
        let end = text[host_start..]
            .find(|c: char| c.is_whitespace() || c == '>')
            .map(|n| host_start + n)
            .unwrap_or(text.len());
        last = Some(UrlSpan { start, end });
    }
    last
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let has_homepage = control
        .source()
        .and_then(|s| s.as_deb822().get("Homepage"))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if has_homepage {
        return Ok(Vec::new());
    }

    let mut diagnostics = Vec::new();
    for binary in control.binaries() {
        let Some(description) = binary.description() else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };
        // Lintian only scans the extended description (everything after
        // the first `\n`).
        let Some((_, extended)) = description.split_once('\n') else {
            continue;
        };
        let Some(url) = find_possible_homepage(extended) else {
            continue;
        };
        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-possibly-contains-homepage",
            Visibility::Info,
            vec![url],
        );
        diagnostics.push(Diagnostic::with_plans(issue, DESCRIPTION, Vec::new()));
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
        [] => "Report possible Homepage URL in description.".to_string(),
        [pkg] => format!("Report possible Homepage URL in description of {}.", pkg),
        _ => format!(
            "Report possible Homepage URL in descriptions of {}.",
            packages.join(", ")
        ),
    }
}

declare_detector! {
    name: "description-possibly-contains-homepage",
    tags: ["description-possibly-contains-homepage"],
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

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect(&ws, &FixerPreferences::default())
    }

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = FsWorkspace::new(base, Some("test".into()), Some(version));
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_has_homepage_keyword_simple() {
        assert!(has_homepage_keyword("See the homepage for more."));
        assert!(has_homepage_keyword("The upstream URL."));
        assert!(has_homepage_keyword("Visit the WEBSITE."));
    }

    #[test]
    fn test_has_homepage_keyword_phrases() {
        assert!(has_homepage_keyword("See the web site."));
        assert!(has_homepage_keyword("See the Home   Page."));
        assert!(has_homepage_keyword("For more info, visit..."));
        assert!(has_homepage_keyword("For further information..."));
    }

    #[test]
    fn test_has_homepage_keyword_absent() {
        assert!(!has_homepage_keyword("A simple description."));
        assert!(!has_homepage_keyword("Nothing about links here."));
    }

    #[test]
    fn test_find_first_url() {
        assert_eq!(
            find_first_url("Visit https://example.com/ for details."),
            Some("https://example.com/".to_string())
        );
        assert_eq!(
            find_first_url("Here: http://a.example.org/foo and later."),
            Some("http://a.example.org/foo".to_string())
        );
    }

    #[test]
    fn test_find_first_url_requires_word_boundary() {
        // The URL must start at a word boundary (lintian's `\b`).
        assert_eq!(find_first_url("prefixhttp://example.com/"), None);
    }

    #[test]
    fn test_find_first_url_requires_alnum_host() {
        // Host must start with [a-z0-9]; a leading punctuation character
        // disqualifies it.
        assert_eq!(find_first_url("http://-foo/"), None);
    }

    #[test]
    fn test_find_first_url_ftp_ignored() {
        assert_eq!(find_first_url("ftp://example.com/"), None);
    }

    #[test]
    fn test_find_trailing_url() {
        assert_eq!(
            find_trailing_url("Some description. https://example.com/foo"),
            Some("https://example.com/foo".to_string())
        );
        // The trailing `.` is part of `[^>\s]+`, so it ends up in the URL.
        // Lintian's `>?\.?\s*\z` constrains position, not the capture.
        assert_eq!(
            find_trailing_url("See https://example.com/foo."),
            Some("https://example.com/foo.".to_string())
        );
        // When wrapped in angle brackets, the capture stops at `>`.
        assert_eq!(
            find_trailing_url("See <https://example.com/foo>"),
            Some("https://example.com/foo".to_string())
        );
    }

    #[test]
    fn test_find_trailing_url_only_when_at_end() {
        // The trailing-URL branch does not fire when the URL is followed
        // by more content.
        assert_eq!(
            find_trailing_url("See https://example.com/ and other stuff."),
            None
        );
    }

    #[test]
    fn test_find_possible_homepage_keyword() {
        // Lintian's regex includes the trailing `.` in the URL because
        // its `[^>\s]+` greedy match stops only at whitespace or `>`.
        let desc = "This is cool. See the homepage at https://example.com/.";
        assert_eq!(
            find_possible_homepage(desc),
            Some("https://example.com/.".to_string())
        );
    }

    #[test]
    fn test_find_possible_homepage_trailing_only() {
        // No trigger keyword, but a trailing URL still triggers the tag.
        let desc = "A short package. https://example.com/foo";
        assert_eq!(
            find_possible_homepage(desc),
            Some("https://example.com/foo".to_string())
        );
    }

    #[test]
    fn test_find_possible_homepage_none() {
        // No URL at all.
        assert_eq!(
            find_possible_homepage("Plain description, nothing here."),
            None
        );
        // URL mid-text but no keyword and not trailing: lintian would not fire.
        assert_eq!(
            find_possible_homepage(
                "Links to https://example.com/ in the middle, but nothing else."
            ),
            None
        );
    }

    #[test]
    fn test_detect_reports_but_does_not_fix() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n Something interesting.\n .\n See the homepage at https://example.com/foo for info.\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        let issue = diagnostics[0].issue.as_ref().unwrap();
        assert_eq!(issue.package.as_deref(), Some("foo"));
        assert_eq!(issue.package_type, Some(PackageType::Binary));
        assert_eq!(
            issue.tag.as_deref(),
            Some("description-possibly-contains-homepage")
        );
        assert_eq!(issue.info.as_deref(), Some("https://example.com/foo"));
        assert!(diagnostics[0].plans.is_empty());

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_detect_skips_when_homepage_declared() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\nHomepage: https://example.com/foo\n\nPackage: foo\nArchitecture: all\nDescription: A tool\n See https://example.com/foo for more info.\n",
        );

        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_detect_trailing_url_without_keyword() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: A tool\n A plain summary.\n .\n https://example.com/foo\n",
        );

        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().info.as_deref(),
            Some("https://example.com/foo"),
        );
    }

    #[test]
    fn test_detect_no_url() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: A tool\n Just plain text, nothing resembling a link.\n",
        );
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_detect_url_in_synopsis_ignored() {
        // Lintian only scans the *extended* description; a URL in the
        // synopsis alone must not trigger the tag.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: A tool from https://example.com/\n Nothing in the body.\n",
        );
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_detect_url_without_keyword_mid_text_ignored() {
        // A URL appearing in the middle of the extended description
        // without any trigger keyword and not at the end: lintian would
        // not fire.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo\nDescription: A tool\n Some text with a link https://example.com/foo in it, continuing on.\n",
        );
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_detect_multiple_binaries() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_control(
            base,
            "Source: foo\n\nPackage: foo1\nDescription: First\n See homepage at https://example.com/one for more.\n\nPackage: foo2\nDescription: Second\n Something boring.\n .\n https://example.com/two\n",
        );
        let diagnostics = detect_in(base).unwrap();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().package.as_deref(),
            Some("foo1"),
        );
        assert_eq!(
            diagnostics[0].issue.as_ref().unwrap().info.as_deref(),
            Some("https://example.com/one"),
        );
        assert_eq!(
            diagnostics[1].issue.as_ref().unwrap().package.as_deref(),
            Some("foo2"),
        );
        assert_eq!(
            diagnostics[1].issue.as_ref().unwrap().info.as_deref(),
            Some("https://example.com/two"),
        );
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(detect_in(tmp.path()).unwrap().is_empty());
    }
}
