use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use deb822_lossless::Deb822;
use regex::Regex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;

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
    let Ok(deb822) = Deb822::from_str(&content) else {
        return Ok(Vec::new());
    };

    let pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9-.]+)").unwrap();

    let mut actions: Vec<Action> = Vec::new();
    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut updated: HashSet<String> = HashSet::new();

    for (idx, para) in deb822.paragraphs().enumerate() {
        let Some(license_field) = para.get("License") else {
            continue;
        };
        if license_field.is_empty() {
            continue;
        }
        let synopsis = license_field
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if synopsis.is_empty() {
            continue;
        }

        let mut changed = false;
        let new_text = pattern.replace_all(&license_field, |caps: &regex::Captures| {
            let path_str = &caps[0];
            let license_name = &caps[1];
            let Some(replacement) = replace_symlink_path(&synopsis, path_str, license_name) else {
                return path_str.to_string();
            };
            let path_without_slash = path_str.trim_start_matches('/').to_string();
            issues.push(LintianIssue::source_with_info(
                "copyright-refers-to-symlink-license",
                vec![path_without_slash.clone()],
            ));
            issues.push(LintianIssue::source_with_info(
                "copyright-refers-to-versionless-license-file",
                vec![path_without_slash],
            ));
            changed = true;
            replacement
        });

        if changed && new_text != license_field {
            updated.insert(synopsis);
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::Index { index: idx },
                field: "License".into(),
                value: new_text.into_owned(),
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let summary = format!(
        "Refer to specific version of license {}.",
        updated.into_iter().collect::<Vec<_>>().join(", ")
    );

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        let problem_description = match issue.tag.as_deref() {
            Some("copyright-refers-to-symlink-license") => {
                "debian/copyright refers to symlink in common-licenses.".to_string()
            }
            Some("copyright-refers-to-versionless-license-file") => {
                "debian/copyright refers to versionless license file.".to_string()
            }
            _ => "debian/copyright refers to non-specific license file.".to_string(),
        };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            problem_description,
            summary.clone(),
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

fn replace_symlink_path(synopsis: &str, path: &str, _license_name: &str) -> Option<String> {
    let base_synopsis = synopsis.trim_end_matches('+');
    let was_link = std::path::Path::new(path).read_link().is_ok();
    let newpath = format!("/usr/share/common-licenses/{}", base_synopsis);
    let newpath_obj = std::path::Path::new(&newpath);
    if !newpath_obj.exists() || newpath_obj.read_link().is_ok() {
        return None;
    }
    if !newpath.starts_with(&format!("{}-", path)) {
        return None;
    }
    if was_link || newpath != path {
        Some(newpath)
    } else {
        None
    }
}

declare_detector! {
    name: "copyright-refers-to-symlink-license",
    tags: ["copyright-refers-to-symlink-license", "copyright-refers-to-versionless-license-file"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_symlink_path_logic() {
        // Without the actual /usr/share/common-licenses files we can't fully
        // test the path-resolution branch of `replace_symlink_path`. The
        // fixer-test fixtures cover the real-system case.
        let pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9-.]+)").unwrap();
        assert!(pattern.is_match("/usr/share/common-licenses/GPL"));
        let caps = pattern
            .captures("/usr/share/common-licenses/GPL-3")
            .unwrap();
        assert_eq!(&caps[1], "GPL-3");
    }
}
