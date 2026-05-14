use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use deb822_lossless::Deb822;
use debian_analyzer::editor::check_generated_file;
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Collect diagnostics for unusual field spacing in `parsed_path` and emit
/// actions targeting `file_for_actions`. Issues are only emitted when
/// `tagged` is true — the second pass over a generated control's rendered
/// file shares the same lintian issue and re-emitting it would
/// double-count.
fn collect_diagnostics(
    file_for_actions: &Path,
    parsed_path: &Path,
    use_typed_control_selectors: bool,
    tagged: bool,
) -> Result<Vec<Diagnostic>, FixerError> {
    let content = std::fs::read_to_string(parsed_path)?;
    let Ok(deb822) = Deb822::from_str(&content) else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    for (idx, paragraph) in deb822.paragraphs().enumerate() {
        let selector = if use_typed_control_selectors {
            typed_selector_for(&paragraph, idx)
        } else {
            generic_selector_for(&paragraph, idx)
        };
        let entry_keys: Vec<(String, usize)> = paragraph
            .entries()
            .filter_map(|e| e.key().map(|k| (k.to_string(), e.line() + 1)))
            .collect();
        for (key, line_number) in entry_keys {
            let probe_paragraph = paragraph.clone();
            let Some(mut probe) = probe_paragraph.get_entry(&key) else {
                continue;
            };
            if !probe.normalize_field_spacing() {
                continue;
            }

            let actions = vec![Action::Deb822(Deb822Action::NormalizeFieldSpacing {
                file: file_for_actions.to_path_buf(),
                paragraph: selector.clone(),
                field: key.clone(),
            })];
            let description = "debian/control has unusual field spacing.";
            let label = "Strip unusual field spacing from debian/control.";
            if tagged {
                let issue = LintianIssue::source_with_info(
                    "debian-control-has-unusual-field-spacing",
                    Visibility::Pedantic,
                    vec![format!("{} [debian/control:{}]", key, line_number)],
                );
                diagnostics.push(Diagnostic::with_actions(issue, description, label, actions));
            } else {
                diagnostics.push(Diagnostic::untagged(description, label, actions));
            }
        }
    }
    Ok(diagnostics)
}

fn typed_selector_for(paragraph: &deb822_lossless::Paragraph, idx: usize) -> ParagraphSelector {
    if let Some(pkg) = paragraph.get("Package") {
        return ParagraphSelector::Binary { package: pkg };
    }
    if paragraph.get("Source").is_some() {
        return ParagraphSelector::Source;
    }
    ParagraphSelector::Index { index: idx }
}

fn generic_selector_for(paragraph: &deb822_lossless::Paragraph, idx: usize) -> ParagraphSelector {
    if let Some(pkg) = paragraph.get("Package") {
        return ParagraphSelector::ByKey {
            field: "Package".into(),
            value: pkg,
        };
    }
    if let Some(src) = paragraph.get("Source") {
        return ParagraphSelector::ByKey {
            field: "Source".into(),
            value: src,
        };
    }
    ParagraphSelector::Index { index: idx }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // check_generated_file walks template paths from disk; fall back
    // to the filesystem escape hatch.
    let Some(base_path) = ws.base_path() else {
        return Ok(Vec::new());
    };
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }

    match check_generated_file(&control_abs) {
        Err(generated_file) => {
            // Generated control file: emit normalisation actions for both
            // the template and the rendered file. The typed control editor
            // diffs by field VALUE on commit, so a whitespace-only fix
            // wouldn't propagate from rendered → template; we have to write
            // each file directly via the generic deb822 applier.
            let Some(template_abs) = &generated_file.template_path else {
                return Ok(Vec::new());
            };
            let template_rel = template_abs
                .strip_prefix(base_path)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|_| template_abs.clone());

            let mut diagnostics = collect_diagnostics(&template_rel, template_abs, false, true)?;
            diagnostics.extend(collect_diagnostics(
                &control_rel,
                &control_abs,
                false,
                false,
            )?);
            Ok(diagnostics)
        }
        Ok(()) => collect_diagnostics(&control_rel, &control_abs, true, true),
    }
}

declare_detector! {
    name: "debian-control-has-unusual-field-spacing",
    tags: ["debian-control-has-unusual-field-spacing"],
    before: ["file-contains-trailing-whitespace"],
    triggers: [
        debian_workspace::Trigger::File("debian/control"),
        debian_workspace::Trigger::File("debian/control.in"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_normalize_double_space() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, "Source: blah\nRecommends:  ${cdbs:Recommends}\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nRecommends: ${cdbs:Recommends}\n",
        );
    }

    #[test]
    fn test_no_changes() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nRecommends: ${cdbs:Recommends}\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_normalize_tab_after_colon() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(&control, "Source: blah\nBuild-Depends:\tpython3\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: python3\n",
        );
    }

    #[test]
    fn test_preserves_continuation_lines() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nBuild-Depends:  cdbs (>= 0.4.123~),\n  anotherline\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nBuild-Depends: cdbs (>= 0.4.123~),\n  anotherline\n",
        );
    }
}
