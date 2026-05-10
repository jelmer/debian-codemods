use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::collections::HashMap;
use std::path::PathBuf;

fn build_typos_map() -> HashMap<String, String> {
    let mut typos = HashMap::new();
    typos.insert("bsd-2".to_string(), "BSD-2-Clause".to_string());
    typos.insert("bsd-3".to_string(), "BSD-3-Clause".to_string());
    typos.insert("bsd-4".to_string(), "BSD-4-Clause".to_string());
    typos.insert("agpl3".to_string(), "AGPL-3".to_string());
    typos.insert("agpl3+".to_string(), "AGPL-3+".to_string());
    typos.insert("lgpl2.1".to_string(), "LGPL-2.1".to_string());
    typos.insert("lgpl2".to_string(), "LGPL-2.0".to_string());
    typos.insert("lgpl3".to_string(), "LGPL-3.0".to_string());
    for i in 1..=3 {
        typos.insert(format!("gplv{}", i), format!("GPL-{}", i));
        typos.insert(format!("gplv{}+", i), format!("GPL-{}+", i));
        typos.insert(format!("gpl{}", i), format!("GPL-{}", i));
        typos.insert(format!("gpl{}+", i), format!("GPL-{}+", i));
    }
    typos
}

/// Replace just the synopsis (the first whitespace-separated token of the
/// License field's first line) with `new_name`, keeping any trailing
/// continuation lines intact.
fn rewrite_license_field(value: &str, new_name: &str) -> String {
    let mut lines = value.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return new_name.to_string();
    };
    let trimmed = first.trim_end_matches(['\r', '\n']);
    let suffix = &first[trimmed.len()..];
    // Keep anything after the first whitespace gap (license exceptions etc.).
    let rest_of_first = match trimmed.split_once(char::is_whitespace) {
        Some((_, rest)) => format!(" {}", rest.trim_start()),
        None => String::new(),
    };
    let mut out = format!("{}{}{}", new_name, rest_of_first, suffix);
    for line in lines {
        out.push_str(line);
    }
    out
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

    let typos = build_typos_map();
    let mut actions: Vec<Action> = Vec::new();
    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut renames: Vec<(String, String)> = Vec::new();

    // We address paragraphs by their position in the deb822 stream. Walk
    // every paragraph and inspect any License field's name token.
    let header_paragraphs = if copyright.header().is_some() { 1 } else { 0 };
    let mut idx = header_paragraphs;
    let mut consume_paragraph = |license_value: Option<String>, line_no: usize| {
        let Some(value) = license_value else {
            idx += 1;
            return;
        };
        let synopsis = value.lines().next().unwrap_or("").trim();
        let name = synopsis.split_whitespace().next().unwrap_or("").to_string();
        if let Some(new_name) = typos.get(&name) {
            issues.push(LintianIssue::source_with_info(
                "invalid-short-name-in-dep5-copyright",
                Visibility::Warning,
                vec![format!("{} [debian/copyright:{}]", name, line_no)],
            ));
            if !renames.iter().any(|(o, _)| o == &name) {
                renames.push((name.clone(), new_name.clone()));
            }
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::Index { index: idx },
                field: "License".into(),
                value: rewrite_license_field(&value, new_name),
            }));
        }
        idx += 1;
    };

    for files_para in copyright.iter_files() {
        let line_no = files_para
            .as_deb822()
            .get_entry("License")
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| files_para.as_deb822().line() + 1);
        consume_paragraph(files_para.as_deb822().get("License"), line_no);
    }
    for license_para in copyright.iter_licenses() {
        let line_no = license_para
            .as_deb822()
            .get_entry("License")
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| license_para.as_deb822().line() + 1);
        consume_paragraph(license_para.as_deb822().get("License"), line_no);
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let summary = format!(
        "Fix invalid short license name in debian/copyright ({})",
        renames
            .iter()
            .map(|(old, new)| format!("{} \u{21d2} {}", old, new))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "Invalid short license name in debian/copyright.",
            summary.clone(),
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

/// Describer that prefers the per-diagnostic plan label (which carries the
/// per-rename summary) over the deduplicated descriptions.
fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    fixed
        .iter()
        .find_map(|(d, _)| d.plans.first().map(|p| p.label.clone()))
        .unwrap_or_default()
}

declare_detector! {
    name: "invalid-short-name-in-dep5-copyright",
    tags: ["invalid-short-name-in-dep5-copyright"],
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
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
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
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: GPL-2+\n\nLicense: GPL-2+\n Full license text here\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_fix_gpl_variant() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let copyright = debian.join("copyright");
        fs::write(
            &copyright,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: gpl2+\n\nLicense: gpl2+\n Full license text here\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Fix invalid short license name in debian/copyright (gpl2+ \u{21d2} GPL-2+)"
        );
        assert_eq!(
            fs::read_to_string(&copyright).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: GPL-2+\n\nLicense: GPL-2+\n Full license text here\n",
        );
    }
}
