use crate::{declare_fixer, FixerError, FixerResult};
use debian_copyright::lossless::Copyright;
use debian_copyright::License;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    if !copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&copyright_path)?;

    let mut copyright: Copyright = content.parse().map_err(|_| FixerError::NoChanges)?;

    // Get the header license
    let header = match copyright.header() {
        Some(h) => h,
        None => return Err(FixerError::NoChanges),
    };
    let header_deb822 = header.as_deb822();
    let header_license_str = match header_deb822.get("License") {
        Some(s) => s,
        None => return Err(FixerError::NoChanges),
    };

    let header_license_lines: Vec<&str> = header_license_str.lines().collect();

    if header_license_lines.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let header_synopsis = header_license_lines[0].trim();

    // Check if header has license text (more than just synopsis)
    let header_has_text = header_license_lines.len() > 1
        && header_license_lines[1..]
            .iter()
            .any(|line| !line.trim().is_empty());

    if !header_has_text {
        return Err(FixerError::NoChanges);
    }

    // Track which licenses are used and which have their own paragraphs
    let mut used_licenses = HashSet::new();
    let mut seen_licenses = HashSet::new();

    // Check Files paragraphs
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(name) = license.name() {
                used_licenses.insert(name.to_string());

                // If the Files paragraph has license text, it's already defined
                if license.text().is_some() {
                    seen_licenses.insert(name.to_string());
                }
            }
        }
    }

    // Check License paragraphs
    for license_para in copyright.iter_licenses() {
        if let Some(name) = license_para.name() {
            seen_licenses.insert(name);
        }
    }

    // Check if the header license is used but doesn't have its own paragraph
    if !used_licenses.contains(header_synopsis) {
        return Err(FixerError::NoChanges);
    }

    if seen_licenses.contains(header_synopsis) {
        return Err(FixerError::NoChanges);
    }

    // Need to add a License paragraph for the header license
    // Parse the header license into a License object
    let header_license = if header_license_lines.len() > 1 {
        let text = header_license_lines[1..].join("\n");
        License::Named(header_synopsis.to_string(), text)
    } else {
        License::Name(header_synopsis.to_string())
    };

    copyright.add_license(&header_license);

    fs::write(&copyright_path, copyright.to_string())?;

    Ok(FixerResult::builder(format!(
        "Add missing license paragraph for {}",
        header_synopsis
    ))
    .fixed_tags(vec!["dep5-file-paragraph-references-header-paragraph"])
    .build())
}

declare_fixer! {
    name: "dep5-file-paragraph-references-header-paragraph",
    tags: ["dep5-file-paragraph-references-header-paragraph"],
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
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: Alicense\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Add missing license paragraph for Alicense"
        );

        let content = fs::read_to_string(debian_dir.join("copyright")).unwrap();
        assert_eq!(
            content,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright:\n 2008-2017 Somebody\nLicense: Alicense\n\nLicense: Alicense\n Some terms\n"
        );
    }

    #[test]
    fn test_no_dep5() {
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

    #[test]
    fn test_no_copyright_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_header_has_no_text() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: Alicense\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_license_paragraph_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: lintrian\nLicense: Alicense\n Some terms\n\nFiles: *\nCopyright: 2008-2017 Somebody\nLicense: Alicense\n\nLicense: Alicense\n Some terms\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
