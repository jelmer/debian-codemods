use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use deb822_lossless::Deb822;
use std::path::PathBuf;
use std::str::FromStr;

/// Old → new field renames in the copyright header. The third tuple
/// element is `true` for fields that can carry multiple values; for
/// those, if the new name already exists, we append rather than rename.
const RENAMES: &[(&str, &str, bool)] = &[
    ("Name", "Upstream-Name", false),
    ("Contact", "Upstream-Contact", true),
    ("Maintainer", "Upstream-Contact", true),
    ("Upstream-Maintainer", "Upstream-Contact", true),
    ("Format-Specification", "Format", false),
];

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
    if !content.starts_with("Format:") && !content.starts_with("Format-Specification:") {
        return Ok(Vec::new());
    }
    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(header) = deb822.paragraphs().next() else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    for &(old_name, new_name, multi_line) in RENAMES {
        let Some(entry) = header.get_entry(old_name) else {
            continue;
        };
        let value = entry.value();
        if value.trim().is_empty() {
            // Empty values are dropped silently — no diagnostic, no
            // user-visible message. Match the original behaviour by
            // emitting a remove-only action with no LintianIssue.
            diagnostics.push(crate::diagnostic::Diagnostic::untagged(
                format!("drop\t{}", old_name),
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::CopyrightHeader,
                    field: old_name.into(),
                })],
            ));
            continue;
        }

        let line_num = entry.line();
        let issue = LintianIssue::source_with_info(
            "obsolete-field-in-dep5-copyright",
            vec![format!(
                "{} {} [debian/copyright:{}]",
                old_name, new_name, line_num
            )],
        );

        // For multi-valued fields where the new name already carries a
        // value, merge the two on the new field rather than letting
        // RenameField clobber.
        let actions = if multi_line && header.get(new_name).is_some() {
            let existing = header.get(new_name).unwrap();
            let combined = format!("{}\n{}", existing.trim(), value.trim());
            vec![
                Action::Deb822(Deb822Action::SetField {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::CopyrightHeader,
                    field: new_name.into(),
                    value: combined,
                }),
                Action::Deb822(Deb822Action::RemoveField {
                    file: copyright_rel.clone(),
                    paragraph: ParagraphSelector::CopyrightHeader,
                    field: old_name.into(),
                }),
            ]
        } else {
            vec![Action::Deb822(Deb822Action::RenameField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::CopyrightHeader,
                from: old_name.into(),
                to: new_name.into(),
            })]
        };

        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("rename\t{} ⇒ {}", old_name, new_name),
            actions,
        ));
    }

    Ok(diagnostics)
}

/// Aggregate the per-rename diagnostics into the historical
/// "Update copyright file header to use current field names (X ⇒ Y, ...)"
/// description. Drop-only diagnostics (empty-value removals) don't
/// surface in the description.
fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let pairs: Vec<&str> = fixed
        .iter()
        .filter_map(|d| d.message.strip_prefix("rename\t"))
        .collect();
    format!(
        "Update copyright file header to use current field names ({})",
        pairs.join(", "),
    )
}

declare_detector! {
    name: "obsolete-field-in-dep5-copyright",
    tags: ["obsolete-field-in-dep5-copyright"],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
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
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nContact: Jelmer <jelmer@samba.org>\nName: lintian-brush\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        // RENAMES order: Name first, then Contact.
        assert_eq!(
            result.description,
            "Update copyright file header to use current field names (Name ⇒ Upstream-Name, Contact ⇒ Upstream-Contact)"
        );

        // Both renames preserve position.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Jelmer <jelmer@samba.org>\nUpstream-Name: lintian-brush\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n"
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multi_line_append() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Existing <existing@example.com>\nContact: New <new@example.com>\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Update copyright file header to use current field names (Contact ⇒ Upstream-Contact)"
        );

        // Existing Upstream-Contact value gets the Contact value appended;
        // Contact is removed. Position of Upstream-Contact is preserved.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Existing <existing@example.com>\n                  New <new@example.com>\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n"
        );
    }
}
