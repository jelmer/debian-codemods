use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, PackageType};
use regex::Regex;
use std::path::PathBuf;

/// Regex taken from /usr/share/lintian/checks/debian/copyright.pm.
fn license_file_re() -> Regex {
    Regex::new(r"(^|/)(COPYING[^/]*|LICENSE)$").unwrap()
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let copyright = match ws.parsed_copyright() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };

    let re = license_file_re();
    let mut diagnostics = Vec::new();

    for para in copyright.iter_files() {
        let files = para.files();
        let kept: Vec<&String> = files.iter().filter(|f| !re.is_match(f)).collect();
        let dropped: Vec<&String> = files.iter().filter(|f| re.is_match(f)).collect();
        if dropped.is_empty() {
            continue;
        }

        let raw_files = para.as_deb822().get("Files").unwrap_or_default();

        // One diagnostic per dropped glob — granular for LSP and for
        // override matching.
        for file_pattern in &dropped {
            let issue = LintianIssue {
                package: None,
                package_type: Some(PackageType::Source),
                tag: Some("license-file-listed-in-debian-copyright".to_string()),
                info: Some(format!("{} [debian/copyright]", file_pattern)),
            };
            diagnostics.push(
                Diagnostic::with_actions(
                    issue,
                    format!("dropped\t{}", file_pattern),
                    // Action set is shared across the per-glob diagnostics:
                    // applying one of them is enough; the rest are no-ops.
                    if kept.is_empty() {
                        vec![Action::Deb822(Deb822Action::RemoveParagraph {
                            file: copyright_rel.clone(),
                            paragraph: ParagraphSelector::CopyrightFiles {
                                glob: raw_files.clone(),
                            },
                        })]
                    } else {
                        // Files: globs are space-separated, matching
                        // debian-copyright's set_files semantics.
                        let new_value = kept
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        vec![Action::Deb822(Deb822Action::SetField {
                            file: copyright_rel.clone(),
                            paragraph: ParagraphSelector::CopyrightFiles {
                                glob: raw_files.clone(),
                            },
                            field: "Files".into(),
                            value: new_value,
                        })]
                    },
                )
                .with_certainty(Certainty::Certain),
            );
        }
    }

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let dropped: Vec<&str> = fixed
        .iter()
        .filter_map(|d| d.message.strip_prefix("dropped\t"))
        .collect();
    format!(
        "Remove listed license files ({}) from copyright.",
        dropped.join(", "),
    )
}

declare_detector! {
    name: "license-file-listed-in-debian-copyright",
    tags: ["license-file-listed-in-debian-copyright"],
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
        adapter.apply(base, "test-package", &version, &FixerPreferences::default())
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
}
