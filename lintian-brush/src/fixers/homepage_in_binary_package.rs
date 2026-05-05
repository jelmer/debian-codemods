use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MESSAGE: &str = "Set Homepage field in Source rather than Binary package.";

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control_path = base_path.join(&control_rel);
    if !control_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_path)?;
    let control: Control = content.parse().map_err(|_| FixerError::NoChanges)?;

    let source_homepage = control
        .source()
        .as_ref()
        .and_then(|s| s.as_deb822().get("Homepage"));

    let binaries_with_homepage: Vec<(String, String)> = control
        .binaries()
        .filter_map(|b| {
            let homepage = b.as_deb822().get("Homepage")?;
            let name = b.name()?;
            Some((name, homepage))
        })
        .collect();

    let mut diagnostics = Vec::new();

    if let Some(source_hp) = &source_homepage {
        // Remove Homepage from binaries that match the source value.
        for (name, hp) in &binaries_with_homepage {
            if hp == source_hp {
                let issue = LintianIssue::source_with_info(
                    "homepage-in-binary-package",
                    vec![name.clone()],
                );
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    MESSAGE,
                    vec![Action::Deb822(Deb822Action::RemoveField {
                        file: control_rel.clone(),
                        paragraph: ParagraphSelector::Binary {
                            package: name.clone(),
                        },
                        field: "Homepage".into(),
                    })],
                ));
            }
        }
    } else {
        // No source Homepage: if all binary Homepages are identical, lift it
        // to the source paragraph and remove it from each binary.
        let unique: HashSet<&str> = binaries_with_homepage
            .iter()
            .map(|(_, hp)| hp.as_str())
            .collect();
        if unique.len() == 1 && !binaries_with_homepage.is_empty() {
            let homepage = binaries_with_homepage[0].1.clone();
            let mut actions = Vec::with_capacity(binaries_with_homepage.len() + 1);
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: "Homepage".into(),
                value: homepage,
            }));
            for (name, _) in &binaries_with_homepage {
                actions.push(Action::Deb822(Deb822Action::RemoveField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Binary {
                        package: name.clone(),
                    },
                    field: "Homepage".into(),
                }));
            }

            // One diagnostic per affected binary, but they share a single
            // ActionPlan: applying any one of them implies applying them
            // all. The default driver picks the first plan of each
            // diagnostic, so list the full coordinated plan on the first
            // diagnostic only and dedupe the rest as informational.
            //
            // Simpler: emit one diagnostic per binary, each carrying the
            // full coordinated set. The applier dedupes equivalent edits
            // (Set then Set the same field value, Remove of an absent
            // field) so the second pass is a no-op.
            for (name, _) in &binaries_with_homepage {
                let issue = LintianIssue::source_with_info(
                    "homepage-in-binary-package",
                    vec![name.clone()],
                );
                diagnostics.push(Diagnostic::with_actions(issue, MESSAGE, actions.clone()));
            }
        }
    }

    Ok(diagnostics)
}

declare_fixer! {
    name: "homepage-in-binary-package",
    tags: ["homepage-in-binary-package"],
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
        FixerImpl.apply(base, "test-package", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_no_source_homepage_same_in_binaries() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nHomepage: https://www.example.com/blah\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah\nDescription: blah2\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.description, MESSAGE);
        assert_eq!(result.fixed_lintian_issues.len(), 2);

        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nHomepage: https://www.example.com/blah\n\nPackage: blah1\nDescription: blah\n\nPackage: blah2\nDescription: blah2\n",
        );
    }

    #[test]
    fn test_source_homepage_matches_binary() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: blah\nMaintainer: Joe <joe@example.com>\nHomepage: https://www.example.com/blah\n\nPackage: blah1\nHomepage: https://www.example.com/blah\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah2\nDescription: blah2\n",
        )
        .unwrap();

        let result = run_apply(temp_dir.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);

        // blah1's Homepage matches the source so it goes; blah2's differs
        // and is left alone.
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: blah\nMaintainer: Joe <joe@example.com>\nHomepage: https://www.example.com/blah\n\nPackage: blah1\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah2\nDescription: blah2\n",
        );
    }

    #[test]
    fn test_no_change_when_different_homepages() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original = "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nHomepage: https://www.example.com/blah1\nDescription: blah\n\nPackage: blah2\nHomepage: https://www.example.com/blah2\nDescription: blah2\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&control_path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_no_homepage() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let original =
            "Source: blah\nMaintainer: Joe <joe@example.com>\n\nPackage: blah1\nDescription: blah\n";
        fs::write(&control_path, original).unwrap();

        assert!(matches!(
            run_apply(temp_dir.path()),
            Err(FixerError::NoChanges)
        ));
    }
}
