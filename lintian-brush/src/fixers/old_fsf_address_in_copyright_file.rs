use crate::declare_detector;
use crate::diagnostic::{
    Action, Deb822Action, Diagnostic, FilesystemAction, ParagraphSelector, TextRange,
};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_copyright::lossless::{encode_field_text, Copyright};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Regex matching "Free Software Foundation" tolerating arbitrary
/// whitespace (including a newline + continuation indent) between words.
///
/// Used to recognise the FSF "write to ..." paragraph that lintian's
/// `old-fsf-address-in-copyright-file` flags. Both historical addresses
/// are matched:
///   * 59 Temple Place - Suite 330, Boston, MA 02111-1307
///   * 51 Franklin Street/St., Fifth Floor, Boston, MA 02110-1301
fn fsf_re() -> Regex {
    Regex::new(r"(?i)Free\s+Software\s+Foundation").unwrap()
}

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

    // Prefer the dep5 path: when the file parses as machine-readable
    // copyright, rewrite each License field that contains an FSF address.
    if let Ok(parsed) = content.parse::<Copyright>() {
        let actions = dep5_actions(&parsed, &copyright_rel);
        if !actions.is_empty() {
            return Ok(vec![build_diagnostic(actions)]);
        }
        // Fall through to the text path if the parser succeeded but no
        // License field carried an address (e.g. the address lived in a
        // free-form Comment field that the parser doesn't model as
        // license text).
    }

    let actions = freeform_actions(content, &copyright_rel);
    if actions.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![build_diagnostic(actions)])
}

fn build_diagnostic(actions: Vec<Action>) -> Diagnostic {
    let issue = LintianIssue::source_with_info(
        "old-fsf-address-in-copyright-file",
        Visibility::Warning,
        vec![],
    );
    Diagnostic::with_actions(
        issue,
        "Old FSF postal address in debian/copyright.",
        "Replace FSF postal address with a reference to https://www.gnu.org/licenses/.",
        actions,
    )
    .with_certainty(Certainty::Certain)
}

/// Build SetField actions for every License field whose text contains an
/// old FSF postal address. Each affected paragraph (Files or standalone
/// License) gets its License value rewritten with the FSF "write to ..."
/// paragraph replaced by a reference to https://www.gnu.org/licenses/.
fn dep5_actions(parsed: &Copyright, copyright_rel: &Path) -> Vec<Action> {
    let mut actions = Vec::new();

    for files_para in parsed.iter_files() {
        let Some(license) = files_para.license() else {
            continue;
        };
        let Some(text) = license.text() else {
            continue;
        };
        let Some(rewritten) = rewrite_license_text(text) else {
            continue;
        };

        // Files paragraphs are selected by their literal Files-glob value
        // (the same convention used by license-file-listed-in-debian-copyright).
        let raw_files = files_para.as_deb822().get("Files").unwrap_or_default();
        actions.push(Action::Deb822(Deb822Action::SetField {
            file: copyright_rel.to_path_buf(),
            paragraph: ParagraphSelector::CopyrightFiles { glob: raw_files },
            field: "License".into(),
            value: license_field_value(license.name(), &rewritten),
        }));
    }

    for license_para in parsed.iter_licenses() {
        let license = license_para.license();
        let Some(text) = license.text() else {
            continue;
        };
        let Some(rewritten) = rewrite_license_text(text) else {
            continue;
        };
        let Some(name) = license.name() else {
            // Anonymous License paragraphs can't be selected by name; skip.
            // They're rare and lintian shouldn't trigger on them in
            // practice.
            continue;
        };
        actions.push(Action::Deb822(Deb822Action::SetField {
            file: copyright_rel.to_path_buf(),
            paragraph: ParagraphSelector::CopyrightLicense {
                name: name.to_string(),
            },
            field: "License".into(),
            value: license_field_value(Some(name), &rewritten),
        }));
    }

    actions
}

/// Build the value for a `License:` field: the optional synopsis (license
/// short-name) followed by the body text with blank lines encoded as `.`
/// paragraph markers. `body` is decoded text (real blank lines), as
/// returned by [`rewrite_license_text`].
fn license_field_value(name: Option<&str>, body: &str) -> String {
    let encoded = encode_field_text(body);
    match name {
        Some(name) => format!("{}\n{}", name, encoded),
        None => encoded,
    }
}

/// Rewrite the body of a License field, returning `Some(new_text)` if it
/// contained an FSF postal-address paragraph that we rewrote, or `None` if
/// no rewrite was needed.
fn rewrite_license_text(text: &str) -> Option<String> {
    let fsf_zip_re = Regex::new(r"\b(?:02111-1307|02110-1301)\b").unwrap();
    if !fsf_zip_re.is_match(text) {
        return None;
    }

    // `text` is decoded License-field text: the deb822 continuation indent
    // is stripped and `.` paragraph markers are already turned into real
    // blank lines, so paragraphs are separated by empty lines here.
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    let mut changed = false;

    while i < lines.len() {
        let line = lines[i];
        if line.contains("You should have received") && line.to_ascii_lowercase().contains("gnu") {
            // Capture the contiguous block up to the next blank line.
            let block_start = i;
            let mut block_end = i;
            while block_end < lines.len() && !lines[block_end].is_empty() {
                block_end += 1;
            }
            let block = &lines[block_start..block_end];
            let block_text: String = block.join("\n");
            if block_text.contains("write to") && fsf_zip_re.is_match(&block_text) {
                out.push(
                    "You should have received a copy of the GNU General Public License".to_string(),
                );
                out.push(
                    "along with this program.  If not, see <https://www.gnu.org/licenses/>."
                        .to_string(),
                );
                i = block_end;
                changed = true;
                continue;
            }
        }
        out.push(line.to_string());
        i += 1;
    }

    if !changed {
        return None;
    }
    Some(out.join("\n"))
}

/// Free-form (non-dep5) fallback: scan the raw text for an FSF-address
/// paragraph and emit a ReplaceText action that swaps it for the gnu.org
/// URL form. Paragraph boundaries are detected by walking lines with the
/// same leading-space indentation as the matched address line, stopping at
/// blank lines or lines beginning with deb822's `.` continuation marker.
fn freeform_actions(content: &str, copyright_rel: &Path) -> Vec<Action> {
    let zip_re = Regex::new(r"\b(?:02111-1307|02110-1301)\b").unwrap();
    let received_re = Regex::new(r"(?i)you should have received a copy of the GNU\b").unwrap();

    let mut actions = Vec::new();
    let mut search_from = 0usize;

    while let Some(m) = zip_re.find_at(content, search_from) {
        let Some((start, end)) = freeform_paragraph_range(content, m.start(), &received_re) else {
            search_from = m.end();
            continue;
        };
        search_from = end;

        let block = &content[start..end];
        if !block.contains("write to") || !fsf_re().is_match(block) {
            continue;
        }

        // start is at the beginning of the "You should have received" line;
        // grab its leading spaces as the per-line indent.
        let indent: String = content[start..].chars().take_while(|c| *c == ' ').collect();

        let replacement = format!(
            "{indent}You should have received a copy of the GNU General Public License\n\
             {indent}along with this program.  If not, see <https://www.gnu.org/licenses/>."
        );

        actions.push(Action::Filesystem(FilesystemAction::ReplaceText {
            file: copyright_rel.to_path_buf(),
            range: TextRange { start, end },
            replacement,
        }));
    }

    actions
}

/// Given a position inside a line containing the FSF zip code, expand the
/// range upward to the "You should have received" line and downward to the
/// end of the address line, requiring the same indentation throughout.
fn freeform_paragraph_range(
    content: &str,
    pos: usize,
    received_re: &Regex,
) -> Option<(usize, usize)> {
    let line_start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = content[pos..]
        .find('\n')
        .map(|p| pos + p)
        .unwrap_or(content.len());

    let indent: String = content[line_start..]
        .chars()
        .take_while(|c| *c == ' ')
        .collect();
    if indent.is_empty() {
        return None;
    }

    let mut block_start = line_start;
    loop {
        if block_start == 0 {
            return None;
        }
        let prev_end = block_start - 1;
        let prev_start = content[..prev_end].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let prev_line = &content[prev_start..prev_end];

        if !prev_line.starts_with(&indent) {
            return None;
        }
        let after_indent = &prev_line[indent.len()..];
        if after_indent.is_empty() || after_indent.trim() == "." {
            return None;
        }
        if after_indent.starts_with(' ') {
            return None;
        }

        block_start = prev_start;
        if received_re.is_match(prev_line) {
            return Some((block_start, line_end));
        }
    }
}

declare_detector! {
    name: "old-fsf-address-in-copyright-file",
    tags: ["old-fsf-address-in-copyright-file"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "License",
            field: "License",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/copyright",
            paragraph_key: "Files",
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
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
    }

    fn write_copyright(base: &Path, content: &str) -> std::path::PathBuf {
        let debian = base.join("debian");
        fs::create_dir(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(&copyright, content).unwrap();
        copyright
    }

    // rewrite_license_text operates on *decoded* License-field text, so
    // paragraph separators are real blank lines, not `.` markers.
    #[test]
    fn test_rewrite_license_text_franklin() {
        let input = "This program is free software ...\n\nYou should have received a copy of the GNU General Public License\nalong with this program; if not, write to the Free Software\nFoundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.\n\nOn Debian systems ...";
        assert_eq!(
            rewrite_license_text(input),
            Some(
                "This program is free software ...\n\nYou should have received a copy of the GNU General Public License\nalong with this program.  If not, see <https://www.gnu.org/licenses/>.\n\nOn Debian systems ..."
                    .to_string()
            ),
        );
    }

    #[test]
    fn test_rewrite_license_text_temple_place() {
        let input = "Header\n\nYou should have received a copy of the GNU General Public\nLicense along with this program; if not, write to the\nFree Software Foundation, Inc., 59 Temple Place - Suite 330,\nBoston, MA 02111-1307, USA.";
        assert_eq!(
            rewrite_license_text(input),
            Some(
                "Header\n\nYou should have received a copy of the GNU General Public License\nalong with this program.  If not, see <https://www.gnu.org/licenses/>."
                    .to_string()
            ),
        );
    }

    #[test]
    fn test_rewrite_license_text_no_address() {
        let input = "Just regular GPL text without any FSF address.";
        assert_eq!(rewrite_license_text(input), None);
    }

    #[test]
    fn test_replace_dep5_franklin_address_end_to_end() {
        let tmp = TempDir::new().unwrap();
        let copyright = write_copyright(
            tmp.path(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: example\n\nFiles: *\nCopyright: 2026 Example\nLicense: GPL-2+\n This program is free software; you can redistribute it and/or modify\n it under the terms of the GNU General Public License as published by\n the Free Software Foundation; either version 2 of the License, or\n (at your option) any later version.\n .\n You should have received a copy of the GNU General Public License\n along with this program; if not, write to the Free Software\n Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.\n .\n On Debian systems, the complete text of the GNU General Public License\n version 2 can be found in \"/usr/share/common-licenses/GPL-2\".\n",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: example\n\nFiles: *\nCopyright: 2026 Example\nLicense: GPL-2+\n This program is free software; you can redistribute it and/or modify\n it under the terms of the GNU General Public License as published by\n the Free Software Foundation; either version 2 of the License, or\n (at your option) any later version.\n .\n You should have received a copy of the GNU General Public License\n along with this program.  If not, see <https://www.gnu.org/licenses/>.\n .\n On Debian systems, the complete text of the GNU General Public License\n version 2 can be found in \"/usr/share/common-licenses/GPL-2\".\n",
        );
    }

    #[test]
    fn test_replace_freeform_franklin_address() {
        let tmp = TempDir::new().unwrap();
        let copyright = write_copyright(
            tmp.path(),
            "License text:\n  You should have received a copy of the GNU General Public License\n  along with this program; if not, write to the Free Software\n  Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.\nMore text.",
        );

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "License text:\n  You should have received a copy of the GNU General Public License\n  along with this program.  If not, see <https://www.gnu.org/licenses/>.\nMore text.",
        );
    }

    #[test]
    fn test_replace_freeform_old_temple_place_address() {
        let tmp = TempDir::new().unwrap();
        let copyright = write_copyright(
            tmp.path(),
            "License text:\n  You should have received a copy of the GNU General Public\n  License along with this program; if not, write to the\n  Free Software Foundation, Inc., 59 Temple Place - Suite 330,\n  Boston, MA 02111-1307, USA.\nMore text.",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.certainty, Some(Certainty::Certain));
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "License text:\n  You should have received a copy of the GNU General Public License\n  along with this program.  If not, see <https://www.gnu.org/licenses/>.\nMore text.",
        );
    }

    #[test]
    fn test_no_old_fsf_address() {
        let tmp = TempDir::new().unwrap();
        write_copyright(
            tmp.path(),
            "Some license text without any FSF address.\nOn Debian systems...",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_already_uses_gnu_org_url() {
        let tmp = TempDir::new().unwrap();
        write_copyright(
            tmp.path(),
            "  You should have received a copy of the GNU General Public License\n  along with this program.  If not, see <https://www.gnu.org/licenses/>.\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
