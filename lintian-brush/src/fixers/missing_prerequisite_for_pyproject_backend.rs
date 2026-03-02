use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::relations::ensure_some_version;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const PREREQUISITE_MAP: &[(&str, &str)] = &[
    ("poetry.core.masonry.api", "python3-poetry-core"),
    ("flit_core.buildapi", "flit"),
    ("setuptools.build_meta", "python3-setuptools"),
];

pub fn run(base_path: &Path, _preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    // Read pyproject.toml
    let pyproject_path = base_path.join("pyproject.toml");
    if !pyproject_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let pyproject_content = fs::read_to_string(&pyproject_path)?;
    let toml: toml_edit::DocumentMut = pyproject_content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse pyproject.toml: {}", e)))?;

    // Get build-backend
    let build_backend = toml
        .get("build-system")
        .and_then(|bs| bs.get("build-backend"))
        .and_then(|bb| bb.as_str())
        .ok_or(FixerError::NoChanges)?;

    // Look up prerequisite
    let prerequisite_map: HashMap<&str, &str> = PREREQUISITE_MAP.iter().copied().collect();
    let prerequisite = prerequisite_map
        .get(build_backend)
        .ok_or(FixerError::NoChanges)?;

    // Check if prerequisite already exists in any build dependency field
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let source = editor.source().ok_or(FixerError::NoChanges)?;

    // Check Build-Depends, Build-Depends-Indep, and Build-Depends-Arch
    for field_name in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
        if let Some(field_value) = source.as_deb822().get(field_name) {
            let (relations, _errors) =
                debian_control::lossless::Relations::parse_relaxed(&field_value, true);

            // Check if prerequisite already exists
            if relations.iter_relations_for(prerequisite).next().is_some() {
                return Err(FixerError::NoChanges);
            }
        }
    }

    let issue = LintianIssue {
        package: source.as_deb822().get("Source").map(|s| s.to_string()),
        package_type: Some(crate::PackageType::Source),
        tag: Some("missing-prerequisite-for-pyproject-backend".to_string()),
        info: Some(format!(
            "{} (does not satisfy {})",
            build_backend, prerequisite
        )),
    };

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    // Add prerequisite to Build-Depends
    let mut source = source;
    let build_depends = source.build_depends().unwrap_or_default();
    let mut new_build_depends = build_depends;
    ensure_some_version(&mut new_build_depends, prerequisite);
    source.set_build_depends(&new_build_depends);

    editor.commit()?;

    Ok(FixerResult::builder(format!(
        "Add missing build-dependency on {}.\n\nThis is necessary for build-backend {} in pyproject.toml",
        prerequisite, build_backend
    ))
    .fixed_issue(issue)
    .build())
}

declare_fixer! {
    name: "missing-prerequisite-for-pyproject-backend",
    tags: ["missing-prerequisite-for-pyproject-backend"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_pyproject_toml() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_adds_missing_prerequisite() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let pyproject_content = r#"[build-system]
requires = [
  "setuptools>=51.0",
  "wheel>=0.36",
  "setuptools_scm>=6.2"
]
build-backend = "setuptools.build_meta"
"#;
        fs::write(base_path.join("pyproject.toml"), pyproject_content).unwrap();

        let control_content = r#"Source: foo
Build-Depends: python3
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result
            .description
            .contains("Add missing build-dependency on python3-setuptools"));

        // Check that python3-setuptools was added to Build-Depends
        let editor = TemplatedControlEditor::open(&control_path).unwrap();
        let source = editor.source().unwrap();
        let build_depends = source.build_depends().unwrap();
        assert_eq!(build_depends.to_string(), "python3, python3-setuptools");
    }

    #[test]
    fn test_prerequisite_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let pyproject_content = r#"[build-system]
requires = ["setuptools>=51.0"]
build-backend = "setuptools.build_meta"
"#;
        fs::write(base_path.join("pyproject.toml"), pyproject_content).unwrap();

        let control_content = r#"Source: foo
Build-Depends: python3, python3-setuptools
"#;
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_unknown_backend() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let pyproject_content = r#"[build-system]
build-backend = "unknown.backend"
"#;
        fs::write(base_path.join("pyproject.toml"), pyproject_content).unwrap();

        let control_content = r#"Source: foo
Build-Depends: python3
"#;
        fs::write(debian_dir.join("control"), control_content).unwrap();

        let preferences = FixerPreferences::default();
        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
