use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use std::fs;
use std::path::Path;
use std::str::FromStr;

const RENAMES: &[(&str, &str, bool)] = &[
    ("Name", "Upstream-Name", false),
    ("Contact", "Upstream-Contact", true),
    ("Maintainer", "Upstream-Contact", true),
    ("Upstream-Maintainer", "Upstream-Contact", true),
    ("Format-Specification", "Format", false),
];

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;

    // Check if it's a machine-readable copyright file
    if !content.starts_with("Format:") && !content.starts_with("Format-Specification:") {
        return Err(FixerError::NoChanges);
    }

    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => {
            // Not a valid deb822 file
            return Err(FixerError::NoChanges);
        }
    };

    let mut paragraphs = deb822.paragraphs();
    let Some(mut header) = paragraphs.next() else {
        return Err(FixerError::NoChanges);
    };

    let mut applied_renames = Vec::new();
    let mut changed = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for &(old_name, new_name, multi_line) in RENAMES {
        if let Some(entry) = header.get_entry(old_name) {
            let value = entry.value();
            if !value.trim().is_empty() {
                // Get line number (1-indexed)
                let line_num = entry.line();

                let issue = LintianIssue::source_with_info(
                    "obsolete-field-in-dep5-copyright",
                    vec![format!("{} {} [debian/copyright:{}]", old_name, new_name, line_num)],
                );

                if !issue.should_fix(base_path) {
                    overridden_issues.push(issue);
                    continue;
                }

                if multi_line {
                    // For multi-line fields, append to existing value
                    if let Some(existing) = header.get(new_name) {
                        let combined = format!("{}\n{}", existing.trim(), value.trim());
                        header.set(new_name, &combined);
                        header.remove(old_name);
                    } else {
                        header.rename(old_name, new_name);
                    }
                } else {
                    // For single-line fields, just rename to preserve position
                    header.rename(old_name, new_name);
                }
                applied_renames.push((old_name, new_name));
                fixed_issues.push(issue);
                changed = true;
            } else {
                header.remove(old_name);
            }
        }
    }

    if !changed {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, deb822.to_string())?;

    let rename_str = applied_renames
        .iter()
        .map(|(old, new)| format!("{} ⇒ {}", old, new))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(FixerResult::builder(format!(
        "Update copyright file header to use current field names ({})",
        rename_str
    ))
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

declare_fixer! {
    name: "obsolete-field-in-dep5-copyright",
    tags: ["obsolete-field-in-dep5-copyright"],
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
    fn test_simple() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nContact: Jelmer <jelmer@samba.org>\nName: lintian-brush\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        // Description order follows RENAMES array order, not file order
        assert_eq!(
            result.description,
            "Update copyright file header to use current field names (Name ⇒ Upstream-Name, Contact ⇒ Upstream-Contact)"
        );

        let content = fs::read_to_string(&copyright_path).unwrap();
        // Note: rename() preserves field order, so Contact->Upstream-Contact stays in position,
        // and Name->Upstream-Name stays in position
        assert_eq!(
            content,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Jelmer <jelmer@samba.org>\nUpstream-Name: lintian-brush\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n"
        );
    }

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_not_machine_readable() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            "This is not a machine-readable copyright file.\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multi_line_append() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Existing <existing@example.com>\nContact: New <new@example.com>\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Update copyright file header to use current field names (Contact ⇒ Upstream-Contact)"
        );

        let content = fs::read_to_string(&copyright_path).unwrap();
        // deb822-lossless aligns continuation lines
        assert_eq!(
            content,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Contact: Existing <existing@example.com>\n                  New <new@example.com>\n\nFiles: *\nLicense: GPL\nCopyright: 2012...\n"
        );
    }
}
