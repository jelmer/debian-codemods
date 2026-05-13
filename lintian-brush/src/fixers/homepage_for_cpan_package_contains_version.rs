use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::PathBuf;
use std::sync::LazyLock;

/// Match a CPAN homepage URL whose last path segment looks like a versioned
/// distribution, e.g.:
///
/// * `https://metacpan.org/release/USER/HTML-Template-2.9`
/// * `https://search.cpan.org/~user/HTML-Template-2.9/`
/// * `https://metacpan.org/release/HTML-Template-2.9/`
///
/// Captures the URL up to (but not including) the trailing `-VERSION`. We
/// strip just that suffix and any trailing slashes — the result is a
/// versionless CPAN page on the same host, which is what lintian wants.
static CPAN_VERSIONED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(https?://(?:search\.cpan\.org|metacpan\.org)/.+?)-[0-9._]+/*$")
        .expect("static regex compiles")
});

/// If `url` matches a versioned CPAN homepage, return its versionless form.
fn strip_cpan_version(url: &str) -> Option<String> {
    CPAN_VERSIONED
        .captures(url)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(homepage) = source.get("Homepage") else {
        return Ok(Vec::new());
    };
    let Some(new_homepage) = strip_cpan_version(&homepage) else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue::source_with_info(
        "homepage-for-cpan-package-contains-version",
        Visibility::Warning,
        vec![homepage.clone()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Homepage field for CPAN package includes a version.",
        "Strip version from CPAN Homepage URL.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
            value: new_homepage,
        })],
    )])
}

declare_detector! {
    name: "homepage-for-cpan-package-contains-version",
    tags: ["homepage-for-cpan-package-contains-version"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Homepage",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{DetectorAdapter, TreeFixerWorkspace};
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    fn detect_in(base: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = TreeFixerWorkspace::new(base, "test", "1.0".parse().unwrap());
        detect(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_strip_cpan_version_metacpan() {
        assert_eq!(
            strip_cpan_version("https://metacpan.org/release/USER/HTML-Template-2.9"),
            Some("https://metacpan.org/release/USER/HTML-Template".to_string())
        );
    }

    #[test]
    fn test_strip_cpan_version_search_cpan() {
        assert_eq!(
            strip_cpan_version("https://search.cpan.org/~user/HTML-Template-2.9/"),
            Some("https://search.cpan.org/~user/HTML-Template".to_string())
        );
    }

    #[test]
    fn test_strip_cpan_version_http() {
        assert_eq!(
            strip_cpan_version("http://search.cpan.org/~user/HTML-Template-2.9/"),
            Some("http://search.cpan.org/~user/HTML-Template".to_string())
        );
    }

    #[test]
    fn test_strip_cpan_version_dotted_version() {
        assert_eq!(
            strip_cpan_version("https://metacpan.org/release/Foo-1.2.3"),
            Some("https://metacpan.org/release/Foo".to_string())
        );
    }

    #[test]
    fn test_strip_cpan_version_unversioned_unchanged() {
        assert_eq!(
            strip_cpan_version("https://metacpan.org/release/HTML-Template"),
            None
        );
        assert_eq!(
            strip_cpan_version("https://search.cpan.org/dist/HTML-Template/"),
            None
        );
    }

    #[test]
    fn test_strip_cpan_version_other_host_unchanged() {
        // Only CPAN/metacpan hosts are in scope.
        assert_eq!(
            strip_cpan_version("https://example.com/release/HTML-Template-2.9"),
            None
        );
    }

    #[test]
    fn test_strips_metacpan_version_in_control() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nHomepage: https://metacpan.org/release/USER/HTML-Template-2.9\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let result = run_apply(base).unwrap();
        assert_eq!(result.description, "Strip version from CPAN Homepage URL.");
        assert_eq!(
            fs::read_to_string(base.join("debian/control")).unwrap(),
            "Source: foo\nHomepage: https://metacpan.org/release/USER/HTML-Template\n\nPackage: foo\nDescription: Foo\n bar\n",
        );
    }

    #[test]
    fn test_no_change_when_unversioned() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\nHomepage: https://metacpan.org/release/HTML-Template\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_when_no_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
        assert!(detect_in(base).unwrap().is_empty());
    }

    #[test]
    fn test_no_change_for_non_cpan_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        let content = "Source: foo\nHomepage: https://example.com/foo-1.2\n\nPackage: foo\nDescription: Foo\n bar\n";
        write_control(base, content);

        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_diagnostic_carries_correct_info() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();
        write_control(
            base,
            "Source: foo\nHomepage: https://metacpan.org/release/USER/HTML-Template-2.9\n\nPackage: foo\nDescription: Foo\n bar\n",
        );

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("homepage-for-cpan-package-contains-version")
        );
        assert_eq!(
            issue.info.as_deref(),
            Some("https://metacpan.org/release/USER/HTML-Template-2.9")
        );
    }
}
