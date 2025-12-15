use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_copyright::lossless::Copyright;
use debian_copyright::License;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn build_typos_map() -> HashMap<String, String> {
    let mut typos = HashMap::new();

    // BSD variants
    typos.insert("bsd-2".to_string(), "BSD-2-Clause".to_string());
    typos.insert("bsd-3".to_string(), "BSD-3-Clause".to_string());
    typos.insert("bsd-4".to_string(), "BSD-4-Clause".to_string());

    // AGPL variants
    typos.insert("agpl3".to_string(), "AGPL-3".to_string());
    typos.insert("agpl3+".to_string(), "AGPL-3+".to_string());

    // LGPL variants
    typos.insert("lgpl2.1".to_string(), "LGPL-2.1".to_string());
    typos.insert("lgpl2".to_string(), "LGPL-2.0".to_string());
    typos.insert("lgpl3".to_string(), "LGPL-3.0".to_string());

    // GPL variants
    for i in 1..=3 {
        typos.insert(format!("gplv{}", i), format!("GPL-{}", i));
        typos.insert(format!("gplv{}+", i), format!("GPL-{}+", i));
        typos.insert(format!("gpl{}", i), format!("GPL-{}", i));
        typos.insert(format!("gpl{}+", i), format!("GPL-{}+", i));
    }

    typos
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;

    let copyright: Copyright = content.parse().map_err(|_| FixerError::NoChanges)?;

    let typos_map = build_typos_map();
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Iterate through Files paragraphs and fix license names
    for mut files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(name) = license.name() {
                if let Some(new_name) = typos_map.get(name) {
                    let line_number = files_para
                        .as_deb822()
                        .get_entry("License")
                        .map(|e| e.line() + 1)
                        .unwrap_or_else(|| files_para.as_deb822().line() + 1);

                    let issue = LintianIssue::source_with_info(
                        "invalid-short-name-in-dep5-copyright",
                        vec![format!("{} [debian/copyright:{}]", name, line_number)],
                    );

                    if !issue.should_fix(base_path) {
                        overridden_issues.push(issue);
                        continue;
                    }

                    if !renames.iter().any(|(old, _)| old == name) {
                        renames.push((name.to_string(), new_name.clone()));
                    }

                    let new_license = if let Some(text) = license.text() {
                        License::Named(new_name.clone(), text.to_string())
                    } else {
                        License::Name(new_name.clone())
                    };
                    files_para.set_license(&new_license);
                    fixed_issues.push(issue);
                }
            }
        }
    }

    // Iterate through License paragraphs and fix license names
    for mut license_para in copyright.iter_licenses() {
        if let Some(name) = license_para.name() {
            if let Some(new_name) = typos_map.get(&name) {
                let line_number = license_para
                    .as_deb822()
                    .get_entry("License")
                    .map(|e| e.line() + 1)
                    .unwrap_or_else(|| license_para.as_deb822().line() + 1);

                let issue = LintianIssue::source_with_info(
                    "invalid-short-name-in-dep5-copyright",
                    vec![format!("{} [debian/copyright:{}]", name, line_number)],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    continue;
                }

                if !renames.iter().any(|(old, _)| old == &name) {
                    renames.push((name.clone(), new_name.clone()));
                }

                let new_license = if let Some(text) = license_para.text() {
                    License::Named(new_name.clone(), text)
                } else {
                    License::Name(new_name.clone())
                };
                license_para.set_license(&new_license);
                fixed_issues.push(issue);
            }
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, copyright.to_string())?;

    let renames_str = renames
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(FixerResult::builder(format!(
        "Fix invalid short license name in debian/copyright ({})",
        renames_str
    ))
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

declare_fixer! {
    name: "invalid-short-name-in-dep5-copyright",
    tags: ["invalid-short-name-in-dep5-copyright"],
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
    fn test_fix_gpl_variant() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: gpl2+\n\nLicense: gpl2+\n Full license text here\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result.description.contains("gpl2+ ⇒ GPL-2+"));

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("License: GPL-2+"));
        assert!(!content.contains("License: gpl2+"));
    }

    #[test]
    fn test_fix_bsd_variant() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: bsd-3\n\nLicense: bsd-3\n Full license text here\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result.description.contains("bsd-3 ⇒ BSD-3-Clause"));

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("License: BSD-3-Clause"));
        assert!(!content.contains("License: bsd-3"));
    }

    #[test]
    fn test_fix_multiple_typos() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: src/*\nCopyright: 2008-2017 Somebody\nLicense: gpl3\n\nFiles: lib/*\nCopyright: 2010 Another\nLicense: lgpl2.1\n\nLicense: gpl3\n GPL-3 text\n\nLicense: lgpl2.1\n LGPL-2.1 text\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert!(result.description.contains("gpl3 ⇒ GPL-3"));
        assert!(result.description.contains("lgpl2.1 ⇒ LGPL-2.1"));

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert!(content.contains("License: GPL-3"));
        assert!(content.contains("License: LGPL-2.1"));
        assert!(!content.contains("License: gpl3"));
        assert!(!content.contains("License: lgpl2.1"));
    }

    #[test]
    fn test_no_changes_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: GPL-2+\n\nLicense: GPL-2+\n Full license text here\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
