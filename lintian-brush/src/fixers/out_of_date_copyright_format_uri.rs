use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
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

    // Two cases: legacy field name (Format-Specification) or current name
    // with a stale value. In both cases the canonical fix is to land at
    // `Format: <CORRECT_FORMAT_URI>` on the header paragraph. For the
    // legacy case we rename in place (preserving position) and then set
    // the value.
    let actions = if header.get("Format-Specification").is_some() {
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
    } else if let Some(value) = header.get("Format") {
        if value == CORRECT_FORMAT_URI {
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

    let line_no = header
        .keys()
        .next()
        .and_then(|k| header.get_entry(&k))
        .map(|e| e.line() + 1)
        .unwrap_or(1);

    let issue = LintianIssue::source_with_info(
        "out-of-date-copyright-format-uri",
        vec![format!("debian/copyright:{}", line_no)],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Use correct machine-readable copyright file URI.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "out-of-date-copyright-format-uri",
    tags: ["out-of-date-copyright-format-uri"],
    after: ["unversioned-copyright-format-uri"],
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
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_updates_format_specification_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();

        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format-Specification: http://svn.debian.org/wsvn/dep/web/deps/dep5.mdwn?op=file&rev=59\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        // Format-Specification renamed to Format in place; value updated.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
        );
    }

    #[test]
    fn test_updates_format_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();

        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: http://old.debian.org/some/old/path\nUpstream-Name: test-package\n\nFiles: *\nCopyright: 2023 Test Author\nLicense: GPL-2+\n",
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
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
