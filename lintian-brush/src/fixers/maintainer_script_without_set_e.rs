use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

const SCRIPTS: &[&str] = &["preinst", "prerm", "postinst", "config", "postrm"];

fn replace_set_e(path: &Path) -> Result<bool, std::io::Error> {
    // Try to read the file
    let content = match fs::read(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    let mut lines: Vec<&[u8]> = content.split_inclusive(|&b| b == b'\n').collect();
    if lines.is_empty() {
        return Ok(false);
    }

    // Check if file already has "set -e\n" anywhere
    if lines.iter().any(|line| *line == b"set -e\n") {
        return Ok(false);
    }

    // Check if first line is "#!/bin/sh -e\n"
    if lines[0] != b"#!/bin/sh -e\n" {
        return Ok(false);
    }

    // Replace shebang
    let mut new_content = Vec::new();
    new_content.extend_from_slice(b"#!/bin/sh\n");

    // Find the right place to insert "set -e"
    // We're looking for the first non-comment, non-blank line (or #DEBHELPER#)
    // Python code: for i, line in enumerate(lines[1:]):
    // When line at lines[1+i] is the first non-comment, we insert at position i
    let mut insert_idx = None;
    for (i, line) in lines[1..].iter().enumerate() {
        let trimmed = line
            .iter()
            .filter(|&&b| b != b'\n' && b != b'\r')
            .copied()
            .collect::<Vec<u8>>();

        // Check if this line is a comment (excluding #DEBHELPER#) or blank
        let is_comment_line = line.starts_with(b"#") && trimmed != b"#DEBHELPER#";
        let is_blank_line = *line == b"\n" || line.iter().all(|&b| b == b'\n' || b == b'\r');

        if !is_comment_line && !is_blank_line {
            // Found the first non-comment, non-blank line (or #DEBHELPER#)
            // This is at index i in lines[1..], which is index i+1 in lines
            // Python inserts at position i (in the enumerate index)
            insert_idx = Some(i);
            break;
        }
    }

    if let Some(i) = insert_idx {
        // i is the enumerate index from lines[1..]
        // lines[i-1] refers to the line just before position i in the original array
        // Python inserts at position i, which means BEFORE the current enumerate element

        let prev_line_blank = if i > 0 {
            lines[i - 1]
                .iter()
                .all(|&b| b == b'\n' || b == b'\r' || b == b' ' || b == b'\t')
        } else {
            // i == 0 means first line after shebang is the target
            // Check lines[0] (the shebang line) - definitely not blank
            false
        };

        // Add all lines from 1 up to (but not including) the insert position i
        for line in &lines[1..i] {
            new_content.extend_from_slice(line);
        }

        // Now insert the two lines in the appropriate order
        if prev_line_blank {
            // lines.insert(i, b"set -e\n")
            // lines.insert(i+1, b"\n")
            new_content.extend_from_slice(b"set -e\n");
            new_content.extend_from_slice(b"\n");
        } else {
            // lines.insert(i, b"\n")
            // lines.insert(i+1, b"set -e\n")
            new_content.extend_from_slice(b"\n");
            new_content.extend_from_slice(b"set -e\n");
        }

        // Add remaining lines from position i onward
        for line in &lines[i..] {
            new_content.extend_from_slice(line);
        }
    } else {
        // No non-comment lines found, shouldn't really happen but handle it
        for line in &lines[1..] {
            new_content.extend_from_slice(line);
        }
    }

    fs::write(path, new_content)?;
    Ok(true)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let debian_dir = base_path.join("debian");
    let mut changed = false;

    for script_name in SCRIPTS {
        let script_path = debian_dir.join(script_name);
        match replace_set_e(&script_path) {
            Ok(true) => changed = true,
            Ok(false) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(FixerError::from(e)),
        }
    }

    if !changed {
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Use set -e rather than passing -e on the shebang-line.")
            .fixed_tags(vec!["maintainer-script-without-set-e"])
            .build(),
    )
}

declare_fixer! {
    name: "maintainer-script-without-set-e",
    tags: ["maintainer-script-without-set-e"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_simple_replacement() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let prerm_path = debian_dir.join("prerm");
        fs::write(&prerm_path, "#!/bin/sh -e\n# Foo\n# bar\n\necho \"blah\"\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&prerm_path).unwrap();
        assert!(content.starts_with("#!/bin/sh\n"));
        assert!(content.contains("set -e"));
        assert!(!content.contains("#!/bin/sh -e"));

        let result = result.unwrap();
        assert_eq!(
            result.description,
            "Use set -e rather than passing -e on the shebang-line."
        );
    }

    #[test]
    fn test_with_debhelper_tag() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let prerm_path = debian_dir.join("prerm");
        fs::write(
            &prerm_path,
            "#!/bin/sh -e\n# Foo\n\n#DEBHELPER#\n\n# bar\n\necho \"blah\"\n",
        )
        .unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&prerm_path).unwrap();
        assert!(content.starts_with("#!/bin/sh\n"));
        assert!(content.contains("set -e"));

        // set -e should be inserted before #DEBHELPER#
        let set_e_pos = content.find("set -e").unwrap();
        let debhelper_pos = content.find("#DEBHELPER#").unwrap();
        assert!(set_e_pos < debhelper_pos);
    }

    #[test]
    fn test_no_change_when_already_has_set_e() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let prerm_path = debian_dir.join("prerm");
        fs::write(&prerm_path, "#!/bin/sh\nset -e\n\necho \"blah\"\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_dash_e() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let prerm_path = debian_dir.join("prerm");
        fs::write(&prerm_path, "#!/bin/sh\n\necho \"blah\"\n").unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_scripts() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
