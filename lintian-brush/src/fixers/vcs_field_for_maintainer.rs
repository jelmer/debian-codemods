use crate::{declare_fixer, FixerError, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use debian_changelog::parseaddr;
use std::collections::HashSet;
use std::path::Path;

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

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;

    let mut source = editor.source().ok_or(FixerError::NoChanges)?;
    let paragraph = source.as_mut_deb822();

    let maintainer = paragraph
        .get("Maintainer")
        .ok_or(FixerError::NoChanges)?
        .to_string();

    let (name, email) = parseaddr(&maintainer);
    let maintainer_name = name.unwrap_or("").to_string();

    // Find matching email in REPLACEMENTS
    let (tag, url_replacements) = REPLACEMENTS
        .iter()
        .find(|(replacement_email, _, _)| email == *replacement_email)
        .map(|(_, tag, url_replacements)| (*tag, *url_replacements))
        .ok_or(FixerError::NoChanges)?;

    // Update all Vcs-* fields
    let field_names: Vec<String> = paragraph
        .keys()
        .filter(|k| k.starts_with("Vcs-"))
        .map(|s| s.to_string())
        .collect();

    let mut changed_fields = HashSet::new();
    for field_name in field_names {
        let Some(value) = paragraph.get(&field_name) else {
            continue;
        };

        let mut url = value.to_string();
        let original_url = url.clone();

        for (old_pattern, new_pattern) in url_replacements {
            url = url.replace(old_pattern, new_pattern);
        }

        if url == original_url {
            continue;
        }

        paragraph.set(&field_name, &url);
        changed_fields.insert(field_name);
    }

    if changed_fields.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let mut changed_fields_sorted: Vec<_> = changed_fields.into_iter().collect();
    changed_fields_sorted.sort();

    Ok(FixerResult::builder(format!(
        "Update fields {} for maintainer {}.",
        changed_fields_sorted.join(", "),
        maintainer_name
    ))
    .fixed_tags(vec![tag])
    .build())
}

declare_fixer! {
    name: "vcs-field-for-maintainer",
    tags: ["old-dpmt-vcs", "old-papt-vcs"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update fields Vcs-Git for maintainer Debian Python Modules Team."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("old-dpmt-vcs".to_string())
        );

        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(control_content.contains("https://salsa.debian.org/python-team/packages/foo"));
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

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update fields Vcs-Git for maintainer Debian Python Applications Team."
        );
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("old-papt-vcs".to_string())
        );

        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(control_content.contains("https://salsa.debian.org/python-team/packages/foo"));
    }
}
