use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue, PackageType};
use debian_control::lossless::Control;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const PREREQUISITE_MAP: &[(&str, &str)] = &[
    ("poetry.core.masonry.api", "python3-poetry-core"),
    ("flit_core.buildapi", "flit"),
    ("setuptools.build_meta", "python3-setuptools"),
];

const SEP: char = '\t';

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let pyproject_path = base_path.join("pyproject.toml");
    if !pyproject_path.exists() {
        return Ok(Vec::new());
    }

    let pyproject_content = std::fs::read_to_string(&pyproject_path)?;
    let toml: toml_edit::DocumentMut = match pyproject_content.parse() {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()),
    };

    let Some(build_backend) = toml
        .get("build-system")
        .and_then(|bs| bs.get("build-backend"))
        .and_then(|bb| bb.as_str())
    else {
        return Ok(Vec::new());
    };

    let prerequisite_map: HashMap<&str, &str> = PREREQUISITE_MAP.iter().copied().collect();
    let Some(prerequisite) = prerequisite_map.get(build_backend) else {
        return Ok(Vec::new());
    };

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

    // Skip if the prerequisite is already a build-dependency in any of
    // the build-depends fields.
    for field in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
        let Some(value) = source.as_deb822().get(field) else {
            continue;
        };
        let (relations, _errors) = debian_control::lossless::Relations::parse_relaxed(&value, true);
        if relations.iter_relations_for(prerequisite).next().is_some() {
            return Ok(Vec::new());
        }
    }

    let issue = LintianIssue {
        package: source.as_deb822().get("Source").map(|s| s.to_string()),
        package_type: Some(PackageType::Source),
        tag: Some("missing-prerequisite-for-pyproject-backend".to_string()),
        info: Some(format!(
            "{} (does not satisfy {})",
            build_backend, prerequisite
        )),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!("{}{}{}{}", prerequisite, SEP, build_backend, SEP),
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: prerequisite.to_string(),
        })],
    )])
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let Some(first) = fixed.first() else {
        return "Add missing build-dependency.".to_string();
    };
    let parts: Vec<&str> = first.message.split(SEP).collect();
    if parts.len() < 2 {
        return "Add missing build-dependency.".to_string();
    }
    format!(
        "Add missing build-dependency on {}.\n\nThis is necessary for build-backend {} in pyproject.toml",
        parts[0], parts[1]
    )
}

declare_fixer! {
    name: "missing-prerequisite-for-pyproject-backend",
    tags: ["missing-prerequisite-for-pyproject-backend"],
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
    fn test_no_pyproject_toml() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_adds_missing_prerequisite() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();

        fs::write(
            tmp.path().join("pyproject.toml"),
            "[build-system]\nrequires = [\"setuptools>=51.0\"]\nbuild-backend = \"setuptools.build_meta\"\n",
        )
        .unwrap();
        let control = debian.join("control");
        fs::write(&control, "Source: foo\nBuild-Depends: python3\n").unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Add missing build-dependency on python3-setuptools.\n\nThis is necessary for build-backend setuptools.build_meta in pyproject.toml",
        );
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: foo\nBuild-Depends: python3, python3-setuptools\n",
        );
    }

    #[test]
    fn test_prerequisite_already_exists() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            tmp.path().join("pyproject.toml"),
            "[build-system]\nrequires = [\"setuptools>=51.0\"]\nbuild-backend = \"setuptools.build_meta\"\n",
        )
        .unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: python3, python3-setuptools\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unknown_backend() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            tmp.path().join("pyproject.toml"),
            "[build-system]\nbuild-backend = \"unknown.backend\"\n",
        )
        .unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: python3\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
