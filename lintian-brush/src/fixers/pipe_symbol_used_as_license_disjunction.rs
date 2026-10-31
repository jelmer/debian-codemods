use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// Replace pipe disjunctions (`A | B`) in a license synopsis with
/// `A or B`, which is what DEP-5 requires. Returns `None` if the
/// synopsis is pipe-free, or if the rewrite would not produce a
/// well-formed DEP-5 license expression (e.g. nested or
/// unparseable constructs where we can't be sure the rewrite is
/// correct).
fn fix_pipe_in_synopsis(synopsis: &str) -> Option<String> {
    let re = regex::Regex::new(r"\s+\|\s+").unwrap();
    if !re.is_match(synopsis) {
        return None;
    }
    let rewritten = re.replace_all(synopsis, " or ").into_owned();
    debian_copyright::LicenseExpr::parse_strict(&rewritten).ok()?;
    Some(rewritten)
}

/// Build the lintian-style info field matching how lintian reports
/// `pipe-symbol-used-as-license-disjunction`.
///
/// Lintian lowercases the identifier and replaces `(`, `)` and `,`
/// with spaces before emitting it as the hint info.
fn lintian_info(synopsis: &str, line: usize) -> String {
    let mut normalised: String = synopsis
        .chars()
        .map(|c| if matches!(c, '(' | ')' | ',') { ' ' } else { c })
        .collect();
    normalised = normalised.to_lowercase();
    format!("{} [debian/copyright:{}]", normalised.trim(), line)
}

/// Rewrite a License field value given a replacement synopsis. The
/// synopsis is the first line; any continuation lines are preserved.
fn rewrite_license_value(existing: &str, new_synopsis: &str) -> String {
    let mut lines = existing.split('\n');
    let _old = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    if rest.is_empty() {
        new_synopsis.to_string()
    } else {
        format!("{}\n{}", new_synopsis, rest.join("\n"))
    }
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

    let mut diagnostics = Vec::new();

    for files_para in copyright.iter_files() {
        let Some(license) = files_para.license() else {
            continue;
        };
        let Some(name) = license.name() else {
            continue;
        };
        let Some(new_synopsis) = fix_pipe_in_synopsis(name) else {
            continue;
        };
        let line_number = files_para
            .as_deb822()
            .entries()
            .find(|e| e.key().as_deref() == Some("License"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| files_para.as_deb822().line() + 1);
        let files_value = files_para.as_deb822().get("Files").unwrap_or_default();
        let raw_license = files_para.as_deb822().get("License").unwrap_or_default();
        let new_value = rewrite_license_value(&raw_license, &new_synopsis);

        let issue = LintianIssue::source_with_info(
            "pipe-symbol-used-as-license-disjunction",
            Visibility::Warning,
            vec![lintian_info(name, line_number)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "License field in debian/copyright uses `|` as disjunction.",
            "Replace `|` with `or` in debian/copyright license disjunction.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::CopyrightFiles { glob: files_value },
                field: "License".into(),
                value: new_value,
            })],
        ));
    }

    for license_para in copyright.iter_licenses() {
        // LicenseParagraph::name() only returns Some when the field has
        // continuation lines; fall back to the full value for the common
        // single-line case.
        let raw_license = license_para.as_deb822().get("License").unwrap_or_default();
        let synopsis = raw_license
            .split_once('\n')
            .map(|(n, _)| n)
            .unwrap_or(raw_license.as_str());
        let Some(new_synopsis) = fix_pipe_in_synopsis(synopsis) else {
            continue;
        };
        let line_number = license_para
            .as_deb822()
            .entries()
            .find(|e| e.key().as_deref() == Some("License"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| license_para.as_deb822().line() + 1);
        let new_value = rewrite_license_value(&raw_license, &new_synopsis);

        let issue = LintianIssue::source_with_info(
            "pipe-symbol-used-as-license-disjunction",
            Visibility::Warning,
            vec![lintian_info(synopsis, line_number)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "License field in debian/copyright uses `|` as disjunction.",
            "Replace `|` with `or` in debian/copyright license disjunction.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::ByKey {
                    field: "License".into(),
                    value: raw_license,
                },
                field: "License".into(),
                value: new_value,
            })],
        ));
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "pipe-symbol-used-as-license-disjunction",
    tags: ["pipe-symbol-used-as-license-disjunction"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
            field: "License",
        },
        debian_workspace::Trigger::Deb822Field {
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
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    #[test]
    fn fix_pipe_in_synopsis_rewrites_pipe() {
        assert_eq!(
            fix_pipe_in_synopsis("GPL-2+ | MIT"),
            Some("GPL-2+ or MIT".to_string())
        );
    }

    #[test]
    fn fix_pipe_in_synopsis_handles_multiple_pipes() {
        assert_eq!(
            fix_pipe_in_synopsis("GPL-2+ | MIT | BSD-3-clause"),
            Some("GPL-2+ or MIT or BSD-3-clause".to_string())
        );
    }

    #[test]
    fn fix_pipe_in_synopsis_leaves_or_untouched() {
        assert_eq!(fix_pipe_in_synopsis("GPL-2+ or MIT"), None);
    }

    #[test]
    fn fix_pipe_in_synopsis_leaves_single_license() {
        assert_eq!(fix_pipe_in_synopsis("GPL-2+"), None);
    }

    #[test]
    fn fix_pipe_in_synopsis_requires_whitespace_around_pipe() {
        // A pipe without surrounding whitespace isn't the disjunction
        // syntax lintian flags.
        assert_eq!(fix_pipe_in_synopsis("GPL-2+|MIT"), None);
    }

    #[test]
    fn fix_pipe_in_synopsis_rejects_malformed_result() {
        // After rewriting, the result must parse as a valid DEP-5
        // license expression - otherwise we might be mangling a
        // construct we don't understand.
        assert_eq!(fix_pipe_in_synopsis("GPL-2+ | MIT |"), None);
        assert_eq!(fix_pipe_in_synopsis("| GPL-2+ | MIT"), None);
    }

    #[test]
    fn fix_pipe_in_synopsis_handles_with_exception() {
        assert_eq!(
            fix_pipe_in_synopsis("GPL-2+ with OpenSSL-exception | MIT"),
            Some("GPL-2+ with OpenSSL-exception or MIT".to_string())
        );
    }

    #[test]
    fn lintian_info_lowercases_and_normalises() {
        assert_eq!(
            lintian_info("GPL-2+ | MIT", 7),
            "gpl-2+ | mit [debian/copyright:7]"
        );
    }

    #[test]
    fn lintian_info_strips_parens_and_commas() {
        assert_eq!(
            lintian_info("(GPL-2+ | MIT)", 7),
            "gpl-2+ | mit [debian/copyright:7]"
        );
    }

    #[test]
    fn rewrite_license_value_preserves_text() {
        assert_eq!(
            rewrite_license_value("GPL-2+ | MIT\n Some text here", "GPL-2+ or MIT"),
            "GPL-2+ or MIT\n Some text here"
        );
    }

    #[test]
    fn rewrite_license_value_single_line() {
        assert_eq!(
            rewrite_license_value("GPL-2+ | MIT", "GPL-2+ or MIT"),
            "GPL-2+ or MIT"
        );
    }

    #[test]
    fn files_paragraph_pipe_is_rewritten() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+ | MIT\n\nLicense: GPL-2+\n Text\n\nLicense: MIT\n Text\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Replace `|` with `or` in debian/copyright license disjunction."
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+ or MIT\n\nLicense: GPL-2+\n Text\n\nLicense: MIT\n Text\n",
        );
    }

    #[test]
    fn standalone_license_paragraph_pipe_is_rewritten() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+ or MIT\n\nLicense: GPL-2+ | MIT\n Dual-licensed.\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+ or MIT\n\nLicense: GPL-2+ or MIT\n Dual-licensed.\n",
        );
    }

    #[test]
    fn no_change_when_no_pipe() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+ or MIT\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn no_change_without_whitespace_around_pipe() {
        // A packaging-style alternative like `pkg1|pkg2` isn't the DEP-5
        // disjunction syntax; back off rather than guess.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+|MIT\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn not_machine_readable_is_a_noop() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Not a machine-readable copyright file.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn missing_copyright_file_is_a_noop() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
