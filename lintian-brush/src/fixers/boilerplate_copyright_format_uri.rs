use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

const CORRECT_FORMAT_URI: &str =
    "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

/// Returns true if the Format URI is a dh_make boilerplate placeholder.
///
/// lintian flags the URI when it contains `VERSIONED_FORMAT_URL` or
/// `rev=REVISION` (the placeholders dh_make leaves behind for the
/// maintainer to fill in).
fn is_boilerplate(uri: &str) -> bool {
    uri.contains("VERSIONED_FORMAT_URL") || uri.contains("rev=REVISION")
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = copyright.header() else {
        return Ok(Vec::new());
    };
    let Some(format) = header.as_deb822().get("Format") else {
        return Ok(Vec::new());
    };

    // lintian extracts the first whitespace-delimited token of the Format
    // value and reports that as the info field.
    let uri = match format.split_whitespace().next() {
        Some(u) => u,
        None => return Ok(Vec::new()),
    };
    if !is_boilerplate(uri) {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "boilerplate-copyright-format-uri",
        Visibility::Warning,
        vec![uri.to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Boilerplate copyright format URI.",
        "Use versioned copyright format URI.",
        vec![Action::Deb822(Deb822Action::SetField {
            file: copyright_rel,
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Format".into(),
            value: CORRECT_FORMAT_URI.into(),
        })],
    )])
}

declare_detector! {
    name: "boilerplate-copyright-format-uri",
    tags: ["boilerplate-copyright-format-uri"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "Format",
        },
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

    #[test]
    fn test_is_boilerplate() {
        assert!(is_boilerplate("<VERSIONED_FORMAT_URL>"));
        assert!(is_boilerplate(
            "http://anonscm.debian.org/viewvc/dep/web/deps/dep5.mdwn?rev=REVISION"
        ));
        assert!(!is_boilerplate(CORRECT_FORMAT_URI));
        assert!(!is_boilerplate(
            "http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
        ));
    }

    #[test]
    fn test_versioned_format_url_placeholder() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: <VERSIONED_FORMAT_URL>\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("boilerplate-copyright-format-uri")
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info.as_deref(),
            Some("<VERSIONED_FORMAT_URL>")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_rev_revision_placeholder() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://anonscm.debian.org/viewvc/dep/web/deps/dep5.mdwn?rev=REVISION\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].info.as_deref(),
            Some("http://anonscm.debian.org/viewvc/dep/web/deps/dep5.mdwn?rev=REVISION")
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_already_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
