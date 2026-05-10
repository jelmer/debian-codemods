use crate::declare_detector;
use crate::diagnostic::{Action, ActionPlan, Diagnostic, FilesystemAction};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use std::path::{Path, PathBuf};

/// Tag for the diagnostic message; used to assemble the final
/// description in `describe_aggregate`.
const TAG_MISSING: char = 'M';
const TAG_OLDER: char = 'O';

fn find_patches_directory(ws: &dyn Workspace) -> Result<Option<PathBuf>, FixerError> {
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(debian_workspace::Error::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(debian_analyzer::patches::rules_find_patches_directory(
        &makefile,
    ))
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.read_file(Path::new("debian/debcargo.toml"))?.is_some() {
        return Ok(Vec::new());
    }

    let Some(current_version) = ws.current_version() else {
        return Ok(Vec::new());
    };

    let format_rel = PathBuf::from("debian/source/format");
    let orig_format = ws.source_format()?;

    if let Some(ref fmt) = orig_format {
        if fmt != "1.0" {
            return Ok(Vec::new());
        }
    }

    let target_format = if current_version.is_native() {
        "3.0 (native)"
    } else {
        let patches_dir = find_patches_directory(ws)?;
        if let Some(ref dir) = patches_dir {
            if dir != &PathBuf::from("debian/patches") {
                return Ok(Vec::new());
            }
        }
        "3.0 (quilt)"
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if orig_format.is_none() {
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info(
                "missing-debian-source-format",
                Visibility::Warning,
                vec![target_format.to_string()],
            ),
            format!("{}{}", TAG_MISSING, target_format),
            "Add missing debian/source/format.",
            Vec::new(),
        ));
    }
    diagnostics.push(Diagnostic::with_actions(
        LintianIssue::source_with_info(
            "older-source-format",
            Visibility::Info,
            vec!["1.0".to_string()],
        ),
        format!("{}{}", TAG_OLDER, target_format),
        format!("Upgrade to newer source format {}.", target_format),
        Vec::new(),
    ));

    // The single Write covers both diagnostics; attach to the first.
    let action = Action::Filesystem(FilesystemAction::Write {
        file: format_rel,
        content: format!("{}\n", target_format).into_bytes(),
    });
    diagnostics[0].plans[0].actions.push(action);

    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[(Diagnostic, ActionPlan)], _actions: &[Action]) -> String {
    // Pull the target format from the first message that carries it.
    let target = fixed
        .iter()
        .find_map(|(d, _)| {
            d.message
                .strip_prefix(TAG_OLDER)
                .or_else(|| d.message.strip_prefix(TAG_MISSING))
        })
        .unwrap_or("3.0 (quilt)");
    if target == "1.0" {
        "Add missing debian/source/format.".to_string()
    } else {
        format!("Upgrade to newer source format {}.", target)
    }
}

declare_detector! {
    name: "source-format",
    tags: ["missing-debian-source-format", "older-source-format"],
    triggers: [
        debian_workspace::Trigger::File("debian/debcargo.toml"),
        debian_workspace::Trigger::File("debian/source/format"),
        debian_workspace::Trigger::File("debian/rules"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
    describe: |fixed, actions| describe_aggregate(fixed, actions),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use debversion::Version;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, version: &Version) -> Result<crate::FixerResult, FixerError> {
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", version, &FixerPreferences::default())
    }

    #[test]
    fn test_no_changes_if_already_modern() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("format"), "3.0 (quilt)\n").unwrap();
        let v: Version = "1.0-1".parse().unwrap();
        assert!(matches!(
            run_apply(tmp.path(), &v),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_upgrade_from_1_0_non_native() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let format_path = source_dir.join("format");
        fs::write(&format_path, "1.0\n").unwrap();

        let v: Version = "1.0-1".parse().unwrap();
        run_apply(tmp.path(), &v).unwrap();
        assert_eq!(fs::read_to_string(&format_path).unwrap(), "3.0 (quilt)\n");
    }

    #[test]
    fn test_upgrade_from_1_0_native() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        let format_path = source_dir.join("format");
        fs::write(&format_path, "1.0\n").unwrap();

        let v: Version = "1.0".parse().unwrap();
        run_apply(tmp.path(), &v).unwrap();
        assert_eq!(fs::read_to_string(&format_path).unwrap(), "3.0 (native)\n");
    }

    #[test]
    fn test_create_missing_format() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let v: Version = "1.0-1".parse().unwrap();
        let result = run_apply(tmp.path(), &v).unwrap();
        let format_path = tmp.path().join("debian/source/format");
        assert_eq!(fs::read_to_string(&format_path).unwrap(), "3.0 (quilt)\n");
        assert!(result
            .description
            .contains("Upgrade to newer source format"));
    }

    #[test]
    fn test_skip_debcargo_package() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("debcargo.toml"), "[package]\n").unwrap();
        let v: Version = "1.0-1".parse().unwrap();
        assert!(matches!(
            run_apply(tmp.path(), &v),
            Err(FixerError::NoChanges)
        ));
    }
}
