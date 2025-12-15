use crate::lintian_overrides::{fix_override_info, map_overrides, LintianOverrides};
use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn find_override_files(base_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        paths.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    let Ok(entries) = fs::read_dir(&debian_dir) else {
        return paths;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if name.to_string_lossy().ends_with(".lintian-overrides") {
            paths.push(path);
        }
    }

    paths
}

fn shorten_path(path: &Path, base_path: &Path) -> String {
    let rel_path = path.strip_prefix(base_path).unwrap_or(path);
    let path_str = rel_path.display().to_string();

    // Abbreviate debian/ to d/
    if let Some(rest) = path_str.strip_prefix("debian/") {
        format!("d/{}", rest)
    } else {
        path_str
    }
}

fn process_overrides_file(path: &Path) -> Result<(bool, Vec<(usize, String)>), FixerError> {
    let content = fs::read_to_string(path)?;
    let parsed = LintianOverrides::parse(&content);
    let overrides = parsed.ok().map_err(|_| FixerError::NoChanges)?;

    let mut changed_lines = Vec::new();

    // Transform override lines
    let updated = map_overrides(&overrides, |line| {
        // Skip comments and empty lines
        if line.is_comment() || line.is_empty() {
            return None;
        }

        let Some(tag_token) = line.tag() else {
            return None;
        };

        let tag = tag_token.text();
        let info = line.info().unwrap_or_default();

        if info.is_empty() {
            return None;
        }

        // Try to fix the override info
        let fixed_info = fix_override_info(tag, &info);

        if fixed_info == info {
            return None;
        }

        // Extract package spec (the entire package spec as stored)
        let package_spec = line.package_spec();
        let package = package_spec.as_ref().and_then(|spec| spec.package_name());
        let package_type = package_spec.as_ref().and_then(|spec| spec.package_type());

        // Return transformed values
        Some((package, package_type, tag.to_string(), Some(fixed_info)))
    });

    // Track which lines were changed by comparing original and updated
    let mut lineno = 1;
    for (orig_line, updated_line) in overrides.lines().zip(updated.lines()) {
        if orig_line.text() != updated_line.text() {
            // Store the original line text (trimmed)
            let orig_text = orig_line.text().trim().to_string();
            changed_lines.push((lineno, orig_text));
        }
        lineno += 1;
    }

    let changed = !changed_lines.is_empty();

    if changed {
        let new_content = updated.text();
        fs::write(path, new_content)?;
    }

    Ok((changed, changed_lines))
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let override_files = find_override_files(base_path);

    if override_files.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_linenos: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut fixed_issues = Vec::new();

    for path in override_files {
        let result = match process_overrides_file(&path) {
            Ok((changed, lines)) if changed => (changed, lines),
            Ok(_) => continue,
            Err(FixerError::NoChanges) => continue,
            Err(e) => return Err(e),
        };

        // Create LintianIssue for each fixed line
        let rel_path = path.strip_prefix(base_path).unwrap_or(&path);
        let mut linenos = Vec::new();
        for (lineno, override_text) in &result.1 {
            let issue = LintianIssue::source_with_info(
                "mismatched-override",
                vec![format!(
                    "{} [{}:{}]",
                    override_text,
                    rel_path.display(),
                    lineno
                )],
            );
            fixed_issues.push(issue);
            linenos.push(*lineno);
        }

        fixed_linenos.insert(path, linenos);
    }

    if fixed_issues.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Generate description
    let description = if fixed_linenos.len() == 1 {
        let (path, linenos) = fixed_linenos.iter().next().unwrap();
        let path_str = shorten_path(path, base_path);
        format!(
            "Update lintian override info format in {} on line {}.",
            path_str,
            linenos_to_ranges(linenos)
        )
    } else {
        // Sort paths for consistent output - source/lintian-overrides comes first
        let mut sorted_paths: Vec<_> = fixed_linenos.iter().collect();
        sorted_paths.sort_by_key(|(path, _)| {
            let rel = path.strip_prefix(base_path).unwrap_or(path);
            let path_str = rel.to_str().unwrap_or("");
            // Sort source/lintian-overrides first, then alphabetically
            if path_str.starts_with("debian/source/") {
                (0, path_str)
            } else {
                (1, path_str)
            }
        });

        let mut details = Vec::new();
        for (path, linenos) in sorted_paths {
            // For multiple files, don't abbreviate and use "+" prefix
            let path_str = path.strip_prefix(base_path).unwrap_or(path);
            details.push(format!(
                "+ {}: line {}",
                path_str.display(),
                linenos_to_ranges(linenos)
            ));
        }
        format!(
            "Update lintian override info to new format:\n{}",
            details.join("\n")
        )
    };

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .build())
}

/// Convert list of line numbers to ranges (e.g., [1, 2, 3, 5, 7, 8, 9] -> "1-3, 5, 7-9")
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

    // Add the last range
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{}-{}", start, end));
    }

    ranges.join(", ")
}

declare_fixer! {
    name: "old-override-info-format",
    tags: ["mismatched-override"],
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
    fn test_linenos_to_ranges() {
        assert_eq!(linenos_to_ranges(&[1]), "1");
        assert_eq!(linenos_to_ranges(&[1, 2, 3]), "1-3");
        assert_eq!(linenos_to_ranges(&[1, 2, 3, 5, 7, 8, 9]), "1-3, 5, 7-9");
        assert_eq!(linenos_to_ranges(&[1, 3, 5, 7]), "1, 3, 5, 7");
    }

    #[test]
    fn test_fix_override_info_debian_rules() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "lintian-brush source: debian-rules-parses-dpkg-parsechangelog debian/rules (line 11)\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update lintian override info format in d/source/lintian-overrides on line 1."
        );

        // Check the file was updated
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert_eq!(
            content,
            "lintian-brush source: debian-rules-parses-dpkg-parsechangelog [debian/rules:11]\n"
        );
    }

    #[test]
    fn test_fix_override_info_multiple() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc usr/lib/python3/dist-packages/crispy_forms/tests/results/bootstrap/*\n\
             python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc usr/lib/python3/dist-packages/crispy_forms/tests/bootstrap/*\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update lintian override info format in d/source/lintian-overrides on line 1-2."
        );

        // Check the file was updated
        let content = fs::read_to_string(&overrides_path).unwrap();
        assert_eq!(
            content,
            "python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc [usr/lib/python3/dist-packages/crispy_forms/tests/results/bootstrap/*]\n\
             python3-django-crispy-forms: package-contains-documentation-outside-usr-share-doc [usr/lib/python3/dist-packages/crispy_forms/tests/bootstrap/*]\n"
        );
    }

    #[test]
    fn test_no_changes_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let source_dir = base_path.join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();

        let overrides_path = source_dir.join("lintian-overrides");
        fs::write(
            &overrides_path,
            "package: some-tag [already-in-new-format]\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_override_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
