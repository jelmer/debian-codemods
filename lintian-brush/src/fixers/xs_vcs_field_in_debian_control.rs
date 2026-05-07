use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

const DESCRIPTION: &str = "Remove unnecessary XS- prefix for Vcs- fields in debian/control.";
const LABEL: &str = "Remove unnecessary XS- prefix for Vcs- fields in debian/control.";

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();

    let xs_vcs_fields: Vec<(String, usize)> = paragraph
        .keys()
        .filter(|key| key.starts_with("XS-Vcs-"))
        .filter_map(|key| {
            paragraph
                .get_entry(&key)
                .map(|entry| (key.to_string(), entry.line() + 1))
        })
        .collect();

    let mut diagnostics = Vec::with_capacity(xs_vcs_fields.len());
    for (xs_field, line_number) in xs_vcs_fields {
        let new_field = xs_field.strip_prefix("XS-").unwrap().to_string();
        let issue = LintianIssue::source_with_info(
            "adopted-extended-field",
            vec![format!(
                "(in section for source) {} [debian/control:{}]",
                xs_field, line_number
            )],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            DESCRIPTION,
            LABEL,
            vec![Action::Deb822(Deb822Action::RenameField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                from: xs_field,
                to: new_field,
            })],
        ));
    }
    Ok(diagnostics)
}

declare_detector! {
    name: "xs-vcs-field-in-debian-control",
    tags: ["adopted-extended-field"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_xs_vcs_git_renamed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: lintian-brush\nXS-Vcs-Git: https://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);
        assert_eq!(result.certainty, None);

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: lintian-brush\nVcs-Git: https://github.com/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        );
    }

    #[test]
    fn test_multiple_xs_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nXS-Vcs-Git: https://git.example.com/repo\nXS-Vcs-Browser: https://git.example.com/repo/browser\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        let result = run_apply(base_path).unwrap();
        assert_eq!(result.description, DESCRIPTION);
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test\nVcs-Git: https://git.example.com/repo\nVcs-Browser: https://git.example.com/repo/browser\n\nPackage: test\nDescription: Test\n Test package\n",
        );
    }

    #[test]
    fn test_no_xs_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(&control_path, "Source: test\nVcs-Git: https://git.example.com/repo\n\nPackage: test\nDescription: Test\n Test package\n").unwrap();

        assert!(matches!(run_apply(base_path), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
