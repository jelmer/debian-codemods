use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let pycompat_path = base_path.join("debian/pycompat");

    if !pycompat_path.exists() {
        return Err(FixerError::NoChanges);
    }

    fs::remove_file(&pycompat_path)?;

    Ok(
        FixerResult::builder("Remove obsolete debian/pycompat file.")
            .fixed_tags(vec!["debian-pycompat-is-obsolete"])
            .certainty(crate::Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "debian-pycompat-is-obsolete",
    tags: ["debian-pycompat-is-obsolete"],
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
    fn test_remove_pycompat() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let pycompat_path = debian_dir.join("pycompat");
        fs::write(&pycompat_path, "2.7\n").unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(result.description, "Remove obsolete debian/pycompat file.");
        assert_eq!(result.certainty, Some(crate::Certainty::Certain));

        assert!(!pycompat_path.exists());
    }

    #[test]
    fn test_no_pycompat() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
