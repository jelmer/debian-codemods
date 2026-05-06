use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use debian_changelog::parseaddr;
use std::path::PathBuf;

const REPLACEMENTS: &[(&str, &str, &[(&str, &str)])] = &[
    (
        "python-modules-team@lists.alioth.debian.org",
        "old-dpmt-vcs",
        &[(
            "https://salsa.debian.org/python-team/modules/",
            "https://salsa.debian.org/python-team/packages/",
        )],
    ),
    (
        "python-apps-team@lists.alioth.debian.org",
        "old-papt-vcs",
        &[(
            "https://salsa.debian.org/python-team/applications/",
            "https://salsa.debian.org/python-team/packages/",
        )],
    ),
];

/// Marker prefix for the maintainer name embedded in the diagnostic's
/// message. The describer pulls it back out so a per-rewrite message stays
/// self-contained for LSP, while the aggregate description matches the
/// historical "for maintainer NAME" wording.
const MAINTAINER_PREFIX: &str = "for maintainer ";

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
    let Some(maintainer) = source.get("Maintainer") else {
        return Ok(Vec::new());
    };

    let (name, email) = parseaddr(&maintainer);
    let maintainer_name = name.unwrap_or("").to_string();

    let Some((tag, url_replacements)) = REPLACEMENTS
        .iter()
        .find(|(replacement_email, _, _)| email == *replacement_email)
        .map(|(_, tag, url_replacements)| (*tag, *url_replacements))
    else {
        return Ok(Vec::new());
    };

    let paragraph = source.as_deb822();
    let field_names: Vec<String> = paragraph
        .keys()
        .filter(|k| k.starts_with("Vcs-"))
        .map(|s| s.to_string())
        .collect();

    let mut diagnostics = Vec::new();
    for field_name in field_names {
        let Some(value) = paragraph.get(&field_name) else {
            continue;
        };
        let mut url = value.clone();
        for (old, new) in url_replacements {
            url = url.replace(old, new);
        }
        if url == value {
            continue;
        }

        let vcs_type = field_name.strip_prefix("Vcs-").unwrap_or(&field_name);
        let issue = LintianIssue::source_with_info(tag, vec![vcs_type.to_string()]);

        diagnostics.push(Diagnostic::with_actions(
            issue,
            // Per-issue message; the describer assembles the aggregate
            // form from the embedded maintainer name.
            format!(
                "Update {} {}{}.",
                field_name, MAINTAINER_PREFIX, maintainer_name
            ),
            vec![Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: field_name,
                value: url,
            })],
        ));
    }

    Ok(diagnostics)
}

/// Custom describer: aggregate every fired diagnostic's field name into a
/// single "Update fields A, B for maintainer NAME." line.
fn describe_aggregate(fixed: &[Diagnostic], actions: &[Action]) -> String {
    // Pull field names out of the actions (one Deb822Action::SetField per
    // diagnostic by construction).
    let mut fields: Vec<&str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Deb822(Deb822Action::SetField { field, .. }) => Some(field.as_str()),
            _ => None,
        })
        .collect();
    fields.sort();
    fields.dedup();

    // Recover the maintainer name from the first message's tail.
    let maintainer_name = fixed
        .first()
        .and_then(|d| d.message.rsplit_once(MAINTAINER_PREFIX))
        .map(|(_, tail)| tail.trim_end_matches('.'))
        .unwrap_or("");

    format!(
        "Update fields {} for maintainer {}.",
        fields.join(", "),
        maintainer_name
    )
}

declare_detector! {
    name: "vcs-field-for-maintainer",
    tags: ["old-dpmt-vcs", "old-papt-vcs"],
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
    fn test_dpmt_vcs_update() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: foo\nMaintainer: Debian Python Modules Team <python-modules-team@lists.alioth.debian.org>\nVcs-Git: https://salsa.debian.org/python-team/modules/foo\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update fields Vcs-Git for maintainer Debian Python Modules Team."
        );
        assert_eq!(result.certainty, None);
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("old-dpmt-vcs")
        );

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: foo\nMaintainer: Debian Python Modules Team <python-modules-team@lists.alioth.debian.org>\nVcs-Git: https://salsa.debian.org/python-team/packages/foo\n",
        );
    }

    #[test]
    fn test_papt_vcs_update() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: foo\nMaintainer: Debian Python Applications Team <python-apps-team@lists.alioth.debian.org>\nVcs-Git: https://salsa.debian.org/python-team/applications/foo\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update fields Vcs-Git for maintainer Debian Python Applications Team."
        );
        assert_eq!(result.certainty, None);
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("old-papt-vcs")
        );

        assert_eq!(
            fs::read_to_string(debian_dir.join("control")).unwrap(),
            "Source: foo\nMaintainer: Debian Python Applications Team <python-apps-team@lists.alioth.debian.org>\nVcs-Git: https://salsa.debian.org/python-team/packages/foo\n",
        );
    }

    #[test]
    fn test_multiple_vcs_fields_aggregated() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: foo\nMaintainer: Debian Python Modules Team <python-modules-team@lists.alioth.debian.org>\nVcs-Git: https://salsa.debian.org/python-team/modules/foo\nVcs-Browser: https://salsa.debian.org/python-team/modules/foo\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        // Aggregated description rather than the per-field default join.
        assert_eq!(
            result.description,
            "Update fields Vcs-Browser, Vcs-Git for maintainer Debian Python Modules Team."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 2);
    }
}
