use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, LintianIssue};
use debian_changelog::get_maintainer_from_env;
use debian_control::lossless::Control;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const SEP: char = '\t';

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_abs = base_path.join(&control_rel);
    if !control_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    if source.as_deb822().contains_key("Maintainer") {
        return Ok(Vec::new());
    }

    let Some((fullname, email)) = get_maintainer_from_env(|s| std::env::var(s).ok()) else {
        return Err(FixerError::Other(
            "Could not determine maintainer from environment".to_string(),
        ));
    };
    let maintainer_value = format!("{} <{}>", fullname, email);

    let issue = LintianIssue::source_with_info(
        "required-field",
        vec!["debian/control Maintainer".to_string()],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        format!("set{}{}", SEP, maintainer_value),
        vec![Action::Deb822(Deb822Action::SetField {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Maintainer".into(),
            value: maintainer_value,
        })],
    )
    .with_certainty(Certainty::Possible)])
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let Some(first) = fixed.first() else {
        return "Set the maintainer field.".to_string();
    };
    if let Some(maintainer) = first
        .message
        .split_once(SEP)
        .filter(|(tag, _)| *tag == "set")
        .map(|(_, v)| v)
    {
        format!("Set the maintainer field to: {}.", maintainer)
    } else {
        "Set the maintainer field.".to_string()
    }
}

declare_fixer! {
    name: "no-maintainer-field",
    tags: ["required-field"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
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
    fn test_maintainer_already_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test\nMaintainer: Existing User <existing@example.com>\n\nPackage: test\nDescription: Test\n Test package\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
