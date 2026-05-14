use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;
use std::str::FromStr;

const CORRECT_FORMAT_URI: &str =
    "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    let deb822 = match deb822_lossless::Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = deb822.paragraphs().next() else {
        return Ok(Vec::new());
    };
    let Some(format) = header.get("Format") else {
        return Ok(Vec::new());
    };

    let is_insecure =
        format.starts_with("http://www.debian.org/doc/packaging-manuals/copyright-format/1.0");
    let is_wiki = format.starts_with("http://wiki.debian.org/Proposals/CopyrightFormat");
    if !is_insecure && !is_wiki {
        return Ok(Vec::new());
    }
    if format == CORRECT_FORMAT_URI {
        return Ok(Vec::new());
    }

    // Both diagnostics share the same fix; the second's actions are
    // idempotent against the first.
    let make_action = || {
        Action::Deb822(Deb822Action::SetField {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Format".into(),
            value: CORRECT_FORMAT_URI.into(),
        })
    };

    let mut diagnostics = Vec::new();
    diagnostics.push(Diagnostic::with_actions(
        LintianIssue::source_with_info(
            "insecure-copyright-format-uri",
            Visibility::Pedantic,
            vec![format.clone()],
        ),
        "Insecure copyright file specification URI.",
        "Use secure copyright file specification URI.",
        vec![make_action()],
    ));
    if is_wiki {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "wiki-copyright-format-uri",
                Visibility::Pedantic,
                vec![format],
            ),
            "Wiki copyright file specification URI.",
            "Use secure copyright file specification URI.",
            vec![make_action()],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "copyright-format-uri",
    tags: ["insecure-copyright-format-uri", "wiki-copyright-format-uri"],
    // Must convert http to https before adding version (unversioned-copyright-format-uri).
    before: ["unversioned-copyright-format-uri"],
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
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_insecure_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("insecure-copyright-format-uri")
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info.as_deref(),
            Some("http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/")
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_wiki_uri() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://wiki.debian.org/Proposals/CopyrightFormat\nUpstream-Name: test\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        // Two issues: the URI is both insecure and wiki-flavoured.
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n",
        );
    }

    #[test]
    fn test_already_secure() {
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
