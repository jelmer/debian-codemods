use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use deb822_lossless::Deb822;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let abs = base_path.join(&copyright_rel);
    if !abs.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&abs)?;
    let deb822 = match Deb822::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };

    let mut diagnostics = Vec::new();

    for paragraph in deb822.paragraphs() {
        let Some(files) = paragraph.get("Files") else {
            continue;
        };
        if !files.contains(',') {
            continue;
        }
        // Bash-style brace expansion uses commas inside `{...}`; leave it alone.
        if files.contains('{') {
            continue;
        }

        let line_no = paragraph
            .entries()
            .find(|e| e.key().as_deref() == Some("Files"))
            .map(|e| e.line() + 1)
            .unwrap_or_else(|| paragraph.line() + 1);

        let issue = LintianIssue::source_with_info(
            "comma-separated-files-in-dep5-copyright",
            vec![format!("Files [debian/copyright:{}]", line_no)],
        );

        // Splitting on commas and joining with newlines yields a
        // multi-line deb822 value where each path lives on its own line.
        let new_value = files
            .split(',')
            .map(|s| s.trim())
            .collect::<Vec<_>>()
            .join("\n");

        diagnostics.push(Diagnostic::with_actions(
            issue,
            "debian/copyright: Replace commas with whitespace to separate items in Files paragraph.",
            vec![Action::Deb822(Deb822Action::SetField {
                file: copyright_rel.clone(),
                paragraph: ParagraphSelector::CopyrightFiles { glob: files },
                field: "Files".into(),
                value: new_value,
            })],
        ));
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "comma-separated-files-in-dep5-copyright",
    tags: ["comma-separated-files-in-dep5-copyright"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        fs::write(
            &path,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nName: apackage\nMaintainer: Joe Maintainer <joe@example.com>\n\nFiles: update-passwd.c, man/*\nCopyright: Joe Maintainer <joe@example.com>\nLicense: GPL-2\n\nFiles: *\nCopyright: Somebody Else <somebody@example.com>\nLicense: GPL-2\n\nLicense: GPL-2\n On Debian and Debian-based systems, a copy of the GNU General Public\n License version 2 is available in /usr/share/common-licenses/GPL-2.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "debian/copyright: Replace commas with whitespace to separate items in Files paragraph."
        );

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nName: apackage\nMaintainer: Joe Maintainer <joe@example.com>\n\nFiles: update-passwd.c\n       man/*\nCopyright: Joe Maintainer <joe@example.com>\nLicense: GPL-2\n\nFiles: *\nCopyright: Somebody Else <somebody@example.com>\nLicense: GPL-2\n\nLicense: GPL-2\n On Debian and Debian-based systems, a copy of the GNU General Public\n License version 2 is available in /usr/share/common-licenses/GPL-2.\n"
        );
    }

    #[test]
    fn test_bash_expansion() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let path = debian.join("copyright");
        let original = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: {foo,bar}/*\nCopyright: Someone\nLicense: GPL-2\n\nLicense: GPL-2\n Text here\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
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

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
