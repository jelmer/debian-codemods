//! Detector for `description-starts-with-package-name`.
//!
//! lintian's `fields/description` check emits this tag when the synopsis
//! (the first line of a binary package's `Description`) starts with the
//! package name, matched case-insensitively and followed by a word
//! boundary (`/^\Q$pkg\E\b/i`). Per Debian Policy 3.4.1 the synopsis
//! should not repeat the package name.
//!
//! Rewriting the synopsis to read well after the package name is removed
//! needs human judgement, so this is report-only: it flags the issue
//! without proposing an automatic fix.

use crate::declare_detector;
use crate::diagnostic::Diagnostic;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;

const DESCRIPTION: &str = "Description synopsis starts with the package name.";

/// Whether `c` is a Perl `\w` character (`[A-Za-z0-9_]`).
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether the synopsis matches lintian's `/^\Q$pkg\E\b/i`: it starts with
/// `package` (case-insensitive) followed by a word boundary.
///
/// A word boundary sits between a `\w` and a non-`\w` character (or the
/// string edge). The boundary check therefore depends on the last
/// character of the package name and the first character following it.
fn starts_with_package_name(synopsis: &str, package: &str) -> bool {
    if package.is_empty() {
        return false;
    }
    let lower_synopsis = synopsis.to_lowercase();
    let lower_package = package.to_lowercase();
    let Some(rest) = lower_synopsis.strip_prefix(&lower_package) else {
        return false;
    };
    // `\b` after the package name: the boundary exists when the last
    // character of the package and the first character of the remainder
    // differ in word-ness. lintian only fires when that boundary is
    // present, so a remainder starting with a word character is only a
    // match if the package ended with a non-word character.
    let last_pkg_word = package.chars().next_back().is_some_and(is_word_char);
    match rest.chars().next() {
        None => last_pkg_word,
        Some(next) => last_pkg_word != is_word_char(next),
    }
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
    let mut diagnostics = Vec::new();

    for binary in control.binaries() {
        let Some(description) = binary.description() else {
            continue;
        };
        let Some(package_name) = binary.name() else {
            continue;
        };
        // The synopsis is the first line of the Description field.
        let synopsis = match description.split_once('\n') {
            Some((s, _)) => s,
            None => description.as_str(),
        };
        if !starts_with_package_name(synopsis, &package_name) {
            continue;
        }

        let issue = LintianIssue::binary_with_info(
            &package_name,
            "description-starts-with-package-name",
            Visibility::Error,
            vec![],
        );
        diagnostics.push(Diagnostic {
            issue: Some(issue),
            message: DESCRIPTION.to_string(),
            certainty: None,
            patch_name: None,
            plans: Vec::new(),
        });
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "description-starts-with-package-name",
    tags: ["description-starts-with-package-name"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn workspace(base: &Path) -> FsWorkspace {
        let version: crate::Version = "1.0".parse().unwrap();
        FsWorkspace::new(base, Some("test".to_string()), Some(version))
    }

    fn write_control(base: &Path, contents: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), contents).unwrap();
    }

    #[test]
    fn test_starts_with_package_name() {
        assert_eq!(starts_with_package_name("foo is a program", "foo"), true);
        // Case-insensitive.
        assert_eq!(starts_with_package_name("Foo is a program", "foo"), true);
        assert_eq!(starts_with_package_name("FOO is a program", "foo"), true);
        // Word boundary against punctuation.
        assert_eq!(starts_with_package_name("foo - a program", "foo"), true);
        assert_eq!(starts_with_package_name("foo: a program", "foo"), true);
        // Package name only.
        assert_eq!(starts_with_package_name("foo", "foo"), true);
    }

    #[test]
    fn test_does_not_start_with_package_name() {
        assert_eq!(
            starts_with_package_name("a program called foo", "foo"),
            false
        );
        // No word boundary: package ends in a word char, next char is too.
        assert_eq!(starts_with_package_name("foobar utility", "foo"), false);
        // Empty package name never matches.
        assert_eq!(starts_with_package_name("anything", ""), false);
    }

    #[test]
    fn test_word_boundary_for_nonword_package_suffix() {
        // Package ends in a non-word char (`+`). `\b` after it needs a
        // following word char, so a following space is not a boundary and
        // lintian does not fire -- matching Perl's `/^\Qg++\E\b/`.
        assert_eq!(
            starts_with_package_name("g++ compiler frontend", "g++"),
            false
        );
        // A following word char does form a boundary.
        assert_eq!(starts_with_package_name("g++compiler", "g++"), true);
    }

    #[test]
    fn test_detects_synopsis_with_package_name() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blah is a great tool\n A longer description.\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("description-starts-with-package-name")
        );
        assert_eq!(issue.visibility, Some(Visibility::Error));
        assert_eq!(issue.package.as_deref(), Some("blah"));
        assert!(diags[0].plans.is_empty());
    }

    #[test]
    fn test_detects_synopsis_only() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blah - widget library\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_ignores_clean_synopsis() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: a great tool\n A longer description.\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_ignores_substring_without_boundary() {
        let tmp = TempDir::new().unwrap();
        // Package "blah", synopsis "blahtool ..." -- no word boundary.
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blahtool manages widgets\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_only_offending_binary_reported() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blah client library\n\nPackage: blah-utils\nDescription: command-line utilities\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].issue.as_ref().unwrap().package.as_deref(),
            Some("blah")
        );
    }

    #[test]
    fn test_report_only_yields_no_changes() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blah is a great tool\n A longer description.\n",
        );

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        let result = crate::builtin_fixers::plan_diagnostics(
            tmp.path(),
            &diags,
            &FixerPreferences::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_honors_override() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: blah\n\nPackage: blah\nDescription: blah is a great tool\n A longer description.\n",
        );
        fs::write(
            tmp.path().join("debian/blah.lintian-overrides"),
            "blah: description-starts-with-package-name\n",
        )
        .unwrap();

        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].issue.as_ref().unwrap().should_fix(tmp.path()),
            false
        );
    }

    #[test]
    fn test_no_description_field() {
        let tmp = TempDir::new().unwrap();
        write_control(tmp.path(), "Source: blah\n\nPackage: blah\n");
        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        let diags = detect(&workspace(tmp.path()), &FixerPreferences::default()).unwrap();
        assert!(diags.is_empty());
    }
}
