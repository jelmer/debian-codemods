use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::PathBuf;

/// The path lintian flags as deprecated. Referencing it triggers
/// `copyright-refers-to-deprecated-bsd-license-file`; the BSD license is
/// short enough that policy 12.5 wants its text inlined instead.
const DEPRECATED_BSD_PATH: &str = "/usr/share/common-licenses/BSD";

/// The standard 3-clause BSD license text, as held by the Regents of the
/// University of California. This is the text that used to live in
/// `/usr/share/common-licenses/BSD`; inlining it directly is the fix policy
/// 12.5 asks for.
const BSD_LICENSE_TEXT: &str = "\
Copyright (c) The Regents of the University of California.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.
3. Neither the name of the University nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
SUCH DAMAGE.";

/// True if the license body already contains the inlined BSD text (rather
/// than only referencing the deprecated common-licenses file). We key off
/// the redistribution clause, which is present in the full text but never
/// in a bare file reference.
fn contains_full_bsd_text(body: &str) -> bool {
    body.contains("Redistribution and use in source and binary forms")
}

/// Build the new `License` field value (`synopsis\n<encoded body>`) with the
/// BSD text inlined, preserving the paragraph's original synopsis.
fn build_license_field(synopsis: &str, body: &str) -> String {
    format!(
        "{}\n{}",
        synopsis,
        debian_copyright::lossless::encode_field_text(body)
    )
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");

    // Only act on machine-readable (DEP-5) copyright files. Free-form
    // copyright files can also trip the tag, but rewriting them safely is
    // ambiguous, so we back off rather than risk an incorrect change.
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => {
            tracing::debug!("debian/copyright is not machine-readable: {}", e);
            return Ok(Vec::new());
        }
    };

    let mut diagnostics = Vec::new();

    for para in copyright.iter_licenses() {
        let Some(synopsis) = para.name() else {
            continue;
        };
        let Some(body) = para.text() else {
            continue;
        };
        if !body.contains(DEPRECATED_BSD_PATH) {
            continue;
        }
        // If the full BSD text is already present, the reference is
        // redundant rather than load-bearing; we don't have a safe rewrite
        // for that shape, so leave it alone.
        if contains_full_bsd_text(&body) {
            continue;
        }

        let action = Action::Deb822(Deb822Action::SetFieldWithIndent {
            file: copyright_rel.clone(),
            paragraph: ParagraphSelector::CopyrightLicense {
                name: synopsis.clone(),
            },
            field: "License".to_string(),
            value: build_license_field(&synopsis, BSD_LICENSE_TEXT),
            indent: IndentPattern::Fixed { spaces: 1 },
        });

        let issue = LintianIssue::source(
            "copyright-refers-to-deprecated-bsd-license-file",
            Visibility::Warning,
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/copyright references deprecated /usr/share/common-licenses/BSD.",
                "Inline the BSD license text instead of referring to the deprecated common-licenses file.",
                vec![action],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "copyright-refers-to-deprecated-bsd-license-file",
    tags: ["copyright-refers-to-deprecated-bsd-license-file"],
    triggers: [debian_workspace::Trigger::File("debian/copyright")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
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

    fn write_copyright(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("copyright"), content).unwrap();
    }

    const REFERENCE_COPYRIGHT: &str = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Somebody <somebody@example.com>
License: BSD

License: BSD
 On Debian systems, the complete text of the BSD license can be found
 in /usr/share/common-licenses/BSD.
";

    #[test]
    fn test_inlines_bsd_text() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_copyright(base, REFERENCE_COPYRIGHT);

        let result = run_apply(base).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            result.fixed_lintian_issues.len(),
            1,
            "expected exactly one fixed issue"
        );
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("copyright-refers-to-deprecated-bsd-license-file"),
        );

        let out = fs::read_to_string(base.join("debian/copyright")).unwrap();
        assert!(
            !out.contains(DEPRECATED_BSD_PATH),
            "deprecated reference should be gone:\n{out}"
        );
        assert!(
            out.contains("Redistribution and use in source and binary forms"),
            "full BSD text should be inlined:\n{out}"
        );
        // The synopsis and Files paragraph are untouched.
        assert!(out.contains("Files: *"));
        assert!(out.contains("License: BSD\n"));
    }

    #[test]
    fn test_detect_reports_warning_severity() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        write_copyright(base, REFERENCE_COPYRIGHT);

        let diags = detect_in(base).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(issue.visibility, Some(Visibility::Warning));
        assert_eq!(
            issue.tag.as_deref(),
            Some("copyright-refers-to-deprecated-bsd-license-file")
        );
    }

    #[test]
    fn test_no_change_when_text_already_inlined() {
        // The reference appears alongside the full license text. The tag
        // would still fire, but we have no safe rewrite, so we back off.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = format!(
            "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Somebody <somebody@example.com>
License: BSD

License: BSD
 {BSD_LICENSE_TEXT}
 .
 See also /usr/share/common-licenses/BSD.
"
        );
        write_copyright(base, &content);

        assert!(detect_in(base).unwrap().is_empty());
        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_bsd_reference() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Somebody <somebody@example.com>
License: GPL-2+
 On Debian systems, see /usr/share/common-licenses/GPL-2.
";
        write_copyright(base, content);

        assert!(detect_in(base).unwrap().is_empty());
        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_for_free_form_copyright() {
        // Non-machine-readable copyright (no Format: header) is left alone:
        // rewriting it safely is ambiguous.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        let content =
            "This package is BSD licensed; see /usr/share/common-licenses/BSD for details.\n";
        write_copyright(base, content);

        assert!(detect_in(base).unwrap().is_empty());
        assert!(matches!(run_apply(base), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(detect_in(tmp.path()).unwrap().is_empty());
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
