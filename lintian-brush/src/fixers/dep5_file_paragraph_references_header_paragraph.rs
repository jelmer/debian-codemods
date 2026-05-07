use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_copyright::lossless::Copyright;
use std::collections::HashSet;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let bytes = match ws.read_file(&copyright_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(Vec::new());
    };
    let copyright: Copyright = match content.parse() {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };

    let Some(header) = copyright.header() else {
        return Ok(Vec::new());
    };
    let header_deb822 = header.as_deb822();
    let Some(license_str) = header_deb822.get("License") else {
        return Ok(Vec::new());
    };
    let lines: Vec<&str> = license_str.lines().collect();
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let synopsis = lines[0].trim().to_string();
    let has_text = lines.len() > 1 && lines[1..].iter().any(|l| !l.trim().is_empty());
    if !has_text {
        return Ok(Vec::new());
    }

    let line_no = header_deb822
        .entries()
        .find(|e| e.key().as_deref() == Some("License"))
        .map(|e| e.line() + 1)
        .unwrap_or_else(|| header_deb822.line() + 1);

    // Track which licenses are referenced and which already have their
    // own License paragraph (or inline License: <name>\n <text>).
    let mut used = HashSet::new();
    let mut defined = HashSet::new();
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(name) = license.name() {
                used.insert(name.to_string());
                if license.text().is_some() {
                    defined.insert(name.to_string());
                }
            }
        }
    }
    for license_para in copyright.iter_licenses() {
        if let Some(name) = license_para.name() {
            defined.insert(name);
        }
    }

    if !used.contains(&synopsis) || defined.contains(&synopsis) {
        return Ok(Vec::new());
    }

    // Build the License paragraph value: synopsis + body lines from the
    // header license. Lines coming out of `lines()` retain the deb822
    // continuation-indent prefix (one leading space); strip it so
    // deb822-lossless re-applies its own indent on render.
    let body: Vec<&str> = lines[1..]
        .iter()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect();
    let new_license_value = format!("{}\n{}", synopsis, body.join("\n"));

    let issue = LintianIssue::source_with_info(
        "dep5-file-paragraph-references-header-paragraph",
        vec![format!("{} [debian/copyright:{}]", synopsis, line_no)],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!(
            "Files paragraph references License '{}' defined in header.",
            synopsis
        ),
        format!("Add missing license paragraph for {}", synopsis),
        vec![Action::Deb822(Deb822Action::AppendParagraph {
            file: copyright_rel,
            fields: vec![("License".into(), new_license_value)],
            // DEP-5 mandates single-space indent for License field bodies.
            indent: Some(1),
        })],
    )])
}

declare_detector! {
    name: "dep5-file-paragraph-references-header-paragraph",
    tags: ["dep5-file-paragraph-references-header-paragraph"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Format",
            field: "License",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "License",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "License",
        },
    ],
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
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: Alicense\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Add missing license paragraph for Alicense"
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: Alicense\n\nLicense: Alicense\n Some terms\n"
        );
    }

    #[test]
    fn test_no_dep5() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_header_has_no_text() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: Alicense\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_license_paragraph_already_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: Alicense\n\nLicense: Alicense\n Some terms\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
