use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, IndentPattern, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::{Path, PathBuf};

fn textwrap_description(text: &str) -> Vec<String> {
    let mut ret = Vec::new();
    let text = text.trim();
    let paras: Vec<&str> = text.split("\n\n").collect();
    for (i, para) in paras.iter().enumerate() {
        if para.contains("\n*") {
            ret.extend(para.lines().map(|s| s.to_string()));
        } else {
            let options =
                textwrap::Options::new(70).word_separator(textwrap::WordSeparator::AsciiSpace);
            let wrapped = textwrap::wrap(para, options);
            ret.extend(wrapped.into_iter().map(|s| s.to_string()));
        }
        if i < paras.len() - 1 {
            ret.push(String::new());
        }
    }
    ret
}

fn format_description(summary: &str, lines: &[String]) -> String {
    let mut result = summary.to_string();
    for line in lines {
        result.push('\n');
        if line.is_empty() {
            result.push('.');
        } else {
            result.push_str(line);
        }
    }
    result
}

fn guess_description(
    base_path: &Path,
    binary_count: usize,
    summary: Option<&str>,
    preferences: &FixerPreferences,
) -> Option<String> {
    if binary_count != 1 {
        return None; // TODO: handle multi-binary packages
    }
    let rt = tokio::runtime::Runtime::new().ok()?;
    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };
    let net_access = preferences.net_access;
    rt.block_on(async {
        let metadata = upstream_ontologist::guess_upstream_metadata(
            base_path,
            trust_package,
            net_access,
            None,
            None,
        )
        .await
        .ok()?;
        let summary = summary.or_else(|| {
            metadata.get("Summary").and_then(|s| {
                if let upstream_ontologist::UpstreamDatum::Summary(t) = &s.datum {
                    Some(t.as_str())
                } else {
                    None
                }
            })
        });
        if let Some(desc_datum) = metadata.get("Description") {
            if let upstream_ontologist::UpstreamDatum::Description(desc_text) = &desc_datum.datum {
                let upstream = textwrap_description(desc_text);
                if let Some(summary) = summary {
                    let lines: Vec<String> = upstream
                        .into_iter()
                        .map(|line| {
                            if line.is_empty() {
                                ".".to_string()
                            } else {
                                line
                            }
                        })
                        .collect();
                    return Some(format_description(summary, &lines));
                } else if upstream.len() == 1 {
                    return Some(upstream[0].trim_end_matches('\n').to_string());
                }
            }
        }
        summary.map(|s| s.to_string())
    })
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let binaries: Vec<_> = control.binaries().collect();
    let binary_count = binaries.len();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut updated: Vec<String> = Vec::new();

    for binary in &binaries {
        let Some(name) = binary.name() else {
            continue;
        };
        let existing = binary.description().unwrap_or_default();

        let (summary, tag, info) = if existing.is_empty() {
            (
                None,
                "required-field",
                vec![format!("(in section for {}) Description", name)],
            )
        } else if existing.trim().lines().count() == 1 {
            (
                Some(existing.lines().next().unwrap_or("").to_string()),
                "extended-description-is-empty",
                vec![],
            )
        } else {
            continue;
        };

        let summary_ref = summary.as_deref();
        let Some(base_path) = ws.base_path() else {
            // upstream-ontologist needs to walk the source tree, which only
            // the tree-mode host can provide. LSP-style hosts have to skip.
            continue;
        };
        let Some(new_description) =
            guess_description(base_path, binary_count, summary_ref, preferences)
        else {
            let issue = LintianIssue::binary_with_info(&name, tag, Visibility::Error, info);
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!("Description is missing for binary package {}.", name),
                    format!("Cannot guess description for binary package {}.", name),
                    vec![],
                )
                .with_certainty(Certainty::Possible),
            );
            continue;
        };
        if new_description == existing {
            continue;
        }

        let issue = LintianIssue::binary_with_info(&name, tag, Visibility::Error, info);
        updated.push(name.clone());
        // DEP-5 mandates a single-space continuation indent for
        // Description; the deb822 default would left-align to the
        // field-name column.
        diagnostics.push(Diagnostic::with_actions(
            issue,
            String::new(),
            format!("Set description for binary package {}.", name),
            vec![Action::Deb822(Deb822Action::SetFieldWithIndent {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Binary { package: name },
                field: "Description".into(),
                value: new_description,
                indent: IndentPattern::Fixed { spaces: 1 },
            })],
        ));
    }

    if diagnostics.is_empty() {
        return Ok(Vec::new());
    }

    updated.sort();
    let summary = format!(
        "Add description for binary packages: {}",
        updated.join(", ")
    );
    for d in &mut diagnostics {
        for plan in &mut d.plans {
            plan.label = summary.clone();
        }
        d.certainty = Some(Certainty::Possible);
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "package-has-no-description",
    tags: ["required-field", "extended-description-is-empty"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Description",
        },
    ],
    cost: crate::detector::DetectorCost::Network,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(
        base: &Path,
        preferences: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test-package".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, preferences)
        }
    }

    #[test]
    fn test_no_changes_when_description_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nMaintainer: Test User <test@example.com>\n\nPackage: test-package\nDescription: Test package\n This is a test package with a proper description.\n",
        )
        .unwrap();
        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_textwrap_description() {
        let text = "This is a long line that should be wrapped at around 79 characters to fit properly in the description field.";
        let lines = textwrap_description(text);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.len() <= 79));
    }

    #[test]
    fn test_format_description() {
        let summary = "Short summary";
        let lines = vec![
            "First line".to_string(),
            "".to_string(),
            "Third line".to_string(),
        ];
        let result = format_description(summary, &lines);
        assert_eq!(result, "Short summary\nFirst line\n.\nThird line");
    }
}
