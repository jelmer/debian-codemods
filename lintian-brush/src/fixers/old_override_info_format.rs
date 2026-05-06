use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, LintianOverridesAction, OverrideLineSelector};
use crate::lintian_overrides::{fix_override_info, LintianOverrides};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn find_override_files(ws: &dyn FixerWorkspace) -> Result<Vec<PathBuf>, FixerError> {
    let mut paths = Vec::new();
    let source_rel = PathBuf::from("debian/source/lintian-overrides");
    if ws.read_file(&source_rel)?.is_some() {
        paths.push(source_rel);
    }
    if let Some(mut entries) = ws.list_dir(Path::new("debian"))? {
        entries.sort();
        for name in entries {
            if name.ends_with(".lintian-overrides") {
                paths.push(PathBuf::from("debian").join(name));
            }
        }
    }
    Ok(paths)
}

fn shorten_path(path: &Path) -> String {
    let path_str = path.display().to_string();
    if let Some(rest) = path_str.strip_prefix("debian/") {
        format!("d/{}", rest)
    } else {
        path_str
    }
}

fn linenos_to_ranges(linenos: &[usize]) -> String {
    if linenos.is_empty() {
        return String::new();
    }
    let mut sorted = linenos.to_vec();
    sorted.sort_unstable();
    let mut ranges = Vec::new();
    let mut start = sorted[0];
    let mut end = sorted[0];
    for &lineno in &sorted[1..] {
        if lineno == end + 1 {
            end = lineno;
        } else {
            if start == end {
                ranges.push(start.to_string());
            } else {
                ranges.push(format!("{}-{}", start, end));
            }
            start = lineno;
            end = lineno;
        }
    }
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{}-{}", start, end));
    }
    ranges.join(", ")
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut fixed_linenos: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut pending: Vec<(LintianIssue, OverrideLineSelector, PathBuf, String)> = Vec::new();

    for rel in find_override_files(ws)? {
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let parsed = LintianOverrides::parse(&content);
        let Ok(overrides) = parsed.ok() else {
            continue;
        };

        for (idx, line) in overrides.lines().enumerate() {
            if line.is_comment() || line.is_empty() {
                continue;
            }
            let Some(tag_token) = line.tag() else {
                continue;
            };
            let tag = tag_token.text();
            let info = line.info().unwrap_or_default();
            if info.is_empty() {
                continue;
            }
            let trimmed_info = info.trim();
            let fixed = fix_override_info(tag, trimmed_info);
            if fixed == trimmed_info {
                continue;
            }

            let lineno = idx + 1;
            let original_text = line.text().trim().to_string();
            let issue = LintianIssue::source_with_info(
                "mismatched-override",
                vec![format!("{} [{}:{}]", original_text, rel.display(), lineno)],
            );
            let package = line.package_spec().and_then(|s| s.package_name());
            let info_for_selector = (!trimmed_info.is_empty()).then(|| trimmed_info.to_string());
            let selector = OverrideLineSelector {
                tag: tag.to_string(),
                info: info_for_selector,
                package,
            };
            fixed_linenos.entry(rel.clone()).or_default().push(lineno);
            pending.push((issue, selector, rel.clone(), fixed));
        }
    }

    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let description = if fixed_linenos.len() == 1 {
        let (path, linenos) = fixed_linenos.iter().next().unwrap();
        format!(
            "Update lintian override info format in {} on line {}.",
            shorten_path(path),
            linenos_to_ranges(linenos)
        )
    } else {
        let mut sorted: Vec<_> = fixed_linenos.iter().collect();
        sorted.sort_by_key(|(path, _)| {
            let path_str = path.to_str().unwrap_or("");
            // debian/source/* sorts first.
            if path_str.starts_with("debian/source/") {
                (0u8, path_str.to_string())
            } else {
                (1u8, path_str.to_string())
            }
        });
        let mut details = Vec::new();
        for (path, linenos) in sorted {
            details.push(format!(
                "+ {}: line {}",
                path.display(),
                linenos_to_ranges(linenos)
            ));
        }
        format!(
            "Update lintian override info to new format:\n{}",
            details.join("\n")
        )
    };

    for (issue, selector, file, new_info) in pending {
        diagnostics.push(Diagnostic::with_actions(
            issue,
            description.clone(),
            vec![Action::LintianOverrides(
                LintianOverridesAction::SetLineInfo {
                    file,
                    selector,
                    new_info,
                },
            )],
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "old-override-info-format",
    tags: ["mismatched-override"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_linenos_to_ranges() {
        assert_eq!(linenos_to_ranges(&[1]), "1");
        assert_eq!(linenos_to_ranges(&[1, 2, 3]), "1-3");
        assert_eq!(linenos_to_ranges(&[1, 2, 3, 5, 7, 8, 9]), "1-3, 5, 7-9");
        assert_eq!(linenos_to_ranges(&[1, 3, 5, 7]), "1, 3, 5, 7");
    }

    #[test]
    fn test_fix_override_info_debian_rules() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "lintian-brush source: debian-rules-parses-dpkg-parsechangelog debian/rules (line 11)\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Update lintian override info format in d/source/lintian-overrides on line 1."
        );
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "lintian-brush source: debian-rules-parses-dpkg-parsechangelog [debian/rules:11]\n",
        );
    }

    #[test]
    fn test_fix_override_info_multiple() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let overrides = source_dir.join("lintian-overrides");
        fs::write(
            &overrides,
            "python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc usr/lib/python3/dist-packages/crispy_forms/tests/results/bootstrap/*\n\
             python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc usr/lib/python3/dist-packages/crispy_forms/tests/bootstrap/*\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Update lintian override info format in d/source/lintian-overrides on line 1-2."
        );
        assert_eq!(
            fs::read_to_string(&overrides).unwrap(),
            "python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc [usr/lib/python3/dist-packages/crispy_forms/tests/results/bootstrap/*]\n\
             python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc [usr/lib/python3/dist-packages/crispy_forms/tests/bootstrap/*]\n",
        );
    }

    #[test]
    fn test_no_changes_needed() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("lintian-overrides"),
            "package: some-tag [already-in-new-format]\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_override_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
