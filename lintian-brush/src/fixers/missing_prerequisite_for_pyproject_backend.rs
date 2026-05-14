use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, FixerPreferences, LintianIssue, PackageType, Visibility};
use debian_workspace::Workspace;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PREREQUISITE_MAP: &[(&str, &str)] = &[
    ("poetry.core.masonry.api", "python3-poetry-core"),
    ("flit_core.buildapi", "flit"),
    ("setuptools.build_meta", "python3-setuptools"),
];

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let pyproject_bytes = match ws.read_file(Path::new("pyproject.toml"))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let pyproject_content = match std::str::from_utf8(&pyproject_bytes) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
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
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
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
        visibility: Some(Visibility::Info),
        tag: Some("missing-prerequisite-for-pyproject-backend".to_string()),
        info: Some(format!(
            "{} (does not satisfy {})",
            build_backend, prerequisite
        )),
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        format!(
            "Build dependency for pyproject.toml backend {} is missing.",
            build_backend
        ),
        format!(
            "Add missing build-dependency on {} for build-backend {}.",
            prerequisite, build_backend
        ),
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: prerequisite.to_string(),
        })],
    )])
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    let Some((first, _)) = fixed.first() else {
        return "Add missing build-dependency.".to_string();
    };
    let Some(issue) = first.issue.as_ref() else {
        return "Add missing build-dependency.".to_string();
    };
    let Some(info) = issue.info.as_deref() else {
        return "Add missing build-dependency.".to_string();
    };
    // info is formatted as `<build_backend> (does not satisfy <prerequisite>)`
    let Some((build_backend, rest)) = info.split_once(" (does not satisfy ") else {
        return "Add missing build-dependency.".to_string();
    };
    let prerequisite = rest.trim_end_matches(')');
    format!(
        "Add missing build-dependency on {}.\n\nThis is necessary for build-backend {} in pyproject.toml",
        prerequisite, build_backend
    )
}

declare_detector! {
    name: "missing-prerequisite-for-pyproject-backend",
    tags: ["missing-prerequisite-for-pyproject-backend"],
    triggers: [
        debian_workspace::Trigger::File("pyproject.toml"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Indep",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends-Arch",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test".into()),
                Some(version.clone()),
            );
            adapter.apply(&ws, &FixerPreferences::default())
        }
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
