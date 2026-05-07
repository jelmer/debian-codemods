use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;
use std::str::FromStr;

const CORRECT_FORMAT_URI: &str =
    "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(Vec::new());
    };
    let deb822 = match deb822_lossless::Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = deb822.paragraphs().next() else {
        return Ok(Vec::new());
    };
    let actions = if header.get("Format-Specification").is_some() {
        // Legacy field name. Rename in place (preserving position) and
        // set to the canonical value.
        vec![
            Action::Deb822(Deb822Action::RenameField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::CopyrightHeader,
                from: "Format-Specification".into(),
                to: "Format".into(),
            }),
            Action::Deb822(Deb822Action::SetField {
                file: copyright_rel,
                paragraph: ParagraphSelector::CopyrightHeader,
                field: "Format".into(),
                value: CORRECT_FORMAT_URI.into(),
            }),
        ]
    } else if let Some(format) = header.get("Format") {
        // Already correct, with or without a trailing slash.
        if format == CORRECT_FORMAT_URI || format == CORRECT_FORMAT_URI.trim_end_matches('/') {
            return Ok(Vec::new());
        }
        vec![Action::Deb822(Deb822Action::SetField {
            file: copyright_rel,
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Format".into(),
            value: CORRECT_FORMAT_URI.into(),
        })]
    } else {
        return Ok(Vec::new());
    };

    let issue = LintianIssue::source_with_info(
        "unversioned-copyright-format-uri",
        vec!["debian/copyright:1".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use versioned copyright format URI.",
        actions,
    )])
}

declare_detector! {
    name: "unversioned-copyright-format-uri",
    tags: ["unversioned-copyright-format-uri"],
    after: ["copyright-format-uri"],
    before: ["out-of-date-copyright-format-uri"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_updates_unversioned_format_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://www.debian.org/doc/packaging-manuals/copyright-format/\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_no_change_when_format_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_format_correct_without_trailing_slash() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_updates_legacy_format_specification() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format-Specification: http://dep.debian.net/deps/dep5\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        // Renamed in place + value set; first line is now the canonical
        // Format URI.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_empty_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("copyright"), "").unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
