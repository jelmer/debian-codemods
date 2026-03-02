use crate::{FixerError, FixerResult, LintianIssue};
use deb822_lossless::Deb822;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;

    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => {
            // Not a machine-readable copyright file
            return Err(FixerError::NoChanges);
        }
    };

    let mut changed = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for mut paragraph in deb822.paragraphs() {
        let Some(files) = paragraph.get("Files") else {
            continue;
        };

        if !files.contains(',') {
            continue;
        }

        if files.contains('{') {
            // Bash-style expansion?
            continue;
        }

        // Get line number for Files field
        let line_no = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Files"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| paragraph.line() + 1);

        let issue = LintianIssue::source_with_info(
            "comma-separated-files-in-dep5-copyright",
            vec![format!("Files [debian/copyright:{}]", line_no)],
        );

        if issue.should_fix(base_path) {
            let entries: Vec<String> = files.split(',').map(|s| s.trim().to_string()).collect();

            let new_value = entries.join("\n");
            paragraph.set("Files", &new_value);
            changed = true;
            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    if !changed {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    fs::write(&copyright_path, deb822.to_string())?;

    Ok(FixerResult::builder(
        "debian/copyright: Replace commas with whitespace to separate items in Files paragraph.",
    )
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .build())
}

declare_fixer! {
    name: "comma-separated-files-in-dep5-copyright",
    tags: ["comma-separated-files-in-dep5-copyright"],
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
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nName: apackage\nMaintainer: Joe Maintainer <joe@example.com>\n\nFiles: update-passwd.c, man/*\nCopyright: Joe Maintainer <joe@example.com>\nLicense: GPL-2\n\nFiles: *\nCopyright: Somebody Else <somebody@example.com>\nLicense: GPL-2\n\nLicense: GPL-2\n On Debian and Debian-based systems, a copy of the GNU General Public\n License version 2 is available in /usr/share/common-licenses/GPL-2.\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "debian/copyright: Replace commas with whitespace to separate items in Files paragraph."
        );

        let content = fs::read_to_string(&copyright_path).unwrap();
        assert_eq!(
            content,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nName: apackage\nMaintainer: Joe Maintainer <joe@example.com>\n\nFiles: update-passwd.c\n       man/*\nCopyright: Joe Maintainer <joe@example.com>\nLicense: GPL-2\n\nFiles: *\nCopyright: Somebody Else <somebody@example.com>\nLicense: GPL-2\n\nLicense: GPL-2\n On Debian and Debian-based systems, a copy of the GNU General Public\n License version 2 is available in /usr/share/common-licenses/GPL-2.\n"
        );
    }

    #[test]
    fn test_bash_expansion() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let copyright_path = debian_dir.join("copyright");
        fs::write(
            &copyright_path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: {foo,bar}/*\nCopyright: Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text here\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let content = fs::read_to_string(&copyright_path).unwrap();
        assert!(content.contains("{foo,bar}/*"));
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
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
