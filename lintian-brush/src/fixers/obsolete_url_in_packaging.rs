use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};

// Include the generated obsolete sites definitions
include!(concat!(env!("OUT_DIR"), "/obsolete_sites.rs"));

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let Some(homepage) = source.as_deb822().get("Homepage") else {
        return Ok(Vec::new());
    };

    let url = match url::Url::parse(&homepage) {
        Ok(u) => u,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(host) = url.host_str() else {
        return Ok(Vec::new());
    };
    if !is_obsolete_site(host) {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "obsolete-url-in-packaging",
        vec![format!("{} [debian/control]", homepage.trim())],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "Drop fields with obsolete URLs.",
        vec![Action::Deb822(Deb822Action::RemoveField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Homepage".into(),
        })],
    )])
}

declare_fixer! {
    name: "obsolete-url-in-packaging",
    tags: ["obsolete-url-in-packaging"],
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
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_remove_obsolete_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomepage: http://foo.tigris.org/\n",
        )
        .unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, "Drop fields with obsolete URLs.");

        assert_eq!(fs::read_to_string(&control_path).unwrap(), "Source: blah\n");
    }

    #[test]
    fn test_no_homepage_field() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: blah\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_non_obsolete_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nHomepage: https://www.example.com/\n",
        )
        .unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }
}
