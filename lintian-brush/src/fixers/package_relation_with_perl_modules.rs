use crate::{Certainty, FixerError, FixerResult, LintianIssue};
use deb822_lossless::Paragraph;
use debian_analyzer::control::TemplatedControlEditor;
use debian_control::lossless::relations::{Entry, Relation, Relations};
use std::path::Path;

/// The dependency field names to check for binary packages.
const BINARY_DEP_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Breaks",
    "Conflicts",
];

/// The dependency field names to check for the source package.
const SOURCE_DEP_FIELDS: &[&str] = &[
    "Build-Depends",
    "Build-Depends-Indep",
    "Build-Depends-Arch",
    "Build-Conflicts",
    "Build-Conflicts-Indep",
    "Build-Conflicts-Arch",
];

/// Scan a relations field for perl-modules entries and create issues.
fn find_perl_modules_issues(
    relations_str: &str,
    field: &str,
    base_path: &Path,
    make_issue: impl Fn(&str, Vec<String>) -> LintianIssue,
    fixed_issues: &mut Vec<LintianIssue>,
    overridden_issues: &mut Vec<LintianIssue>,
) {
    if relations_str.is_empty() {
        return;
    }

    let (relations, _) = Relations::parse_relaxed(relations_str, true);

    for entry in relations.entries() {
        for relation in entry.relations() {
            if relation.name().starts_with("perl-modules") {
                let matched_text = entry.to_string().trim().to_string();
                let issue = make_issue(
                    "package-relation-with-perl-modules",
                    vec![format!("{}: {}", field, matched_text)],
                );
                if issue.should_fix(base_path) {
                    fixed_issues.push(issue);
                } else {
                    overridden_issues.push(issue);
                }
            }
        }
    }
}

/// Apply fixes: replace perl-modules* with perl in a paragraph's dependency field.
fn apply_perl_modules_fix(paragraph: &mut Paragraph, field: &str) {
    let old_value = paragraph.get(field).unwrap_or_default();
    if old_value.is_empty() {
        return;
    }

    let (mut relations, _) = Relations::parse_relaxed(&old_value, true);

    // Collect exact package names and position of first perl-modules* entry
    let mut perl_modules_names: Vec<String> = Vec::new();
    let mut first_position: Option<usize> = None;
    for (idx, entry) in relations.entries().enumerate() {
        for rel in entry.relations() {
            if rel.name().starts_with("perl-modules") {
                if first_position.is_none() {
                    first_position = Some(idx);
                }
                perl_modules_names.push(rel.name());
            }
        }
    }

    if perl_modules_names.is_empty() {
        return;
    }

    let had_perl = relations.has_relation("perl");

    for name in &perl_modules_names {
        relations.drop_dependency(name);
    }

    if !had_perl {
        relations.add_dependency(Entry::from(Relation::simple("perl")), first_position);
    }

    paragraph.set(field, &relations.to_string());
}

fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian").join("control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Check if this is the perl source package itself
    if let Some(source) = editor.source() {
        if source.name().as_deref() == Some("perl") {
            return Err(FixerError::NoChanges);
        }
    }

    // First pass: find all issues and check should_fix
    if let Some(source) = editor.source() {
        for field in SOURCE_DEP_FIELDS {
            let value = source.as_deb822().get(field).unwrap_or_default();
            find_perl_modules_issues(
                &value,
                field,
                base_path,
                |tag, info| LintianIssue::source_with_info(tag, info),
                &mut fixed_issues,
                &mut overridden_issues,
            );
        }
    }

    for binary in editor.binaries() {
        let pkg_name = binary.name().unwrap_or_default();
        for field in BINARY_DEP_FIELDS {
            let value = binary.as_deb822().get(field).unwrap_or_default();
            let pkg = pkg_name.clone();
            find_perl_modules_issues(
                &value,
                field,
                base_path,
                |tag, info| LintianIssue::binary_with_info(&pkg, tag, info),
                &mut fixed_issues,
                &mut overridden_issues,
            );
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Second pass: apply fixes
    if let Some(mut source) = editor.source() {
        for field in SOURCE_DEP_FIELDS {
            apply_perl_modules_fix(source.as_mut_deb822(), field);
        }
    }

    for mut binary in editor.binaries() {
        for field in BINARY_DEP_FIELDS {
            apply_perl_modules_fix(binary.as_mut_deb822(), field);
        }
    }

    editor.commit()?;
    Ok(
        FixerResult::builder("Replace perl-modules dependency with perl.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .certainty(Certainty::Certain)
            .build(),
    )
}

declare_fixer! {
    name: "package-relation-with-perl-modules",
    tags: ["package-relation-with-perl-modules"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_control(content: &str) -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();
        let control_path = debian_dir.join("control");
        fs::write(&control_path, content).unwrap();
        (temp_dir, control_path)
    }

    #[test]
    fn test_build_depends_fix() {
        let (temp_dir, control_path) = setup_control(
            "Source: test-pkg\nBuild-Depends: perl-modules, debhelper-compat (= 13)\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated,
            "Source: test-pkg\nBuild-Depends: perl, debhelper-compat (= 13)\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );
    }

    #[test]
    fn test_build_depends_versioned_perl_modules() {
        let (temp_dir, control_path) = setup_control(
            "Source: test-pkg\nBuild-Depends: perl-modules-5.28, debhelper-compat (= 13)\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated,
            "Source: test-pkg\nBuild-Depends: perl, debhelper-compat (= 13)\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );
    }

    #[test]
    fn test_binary_depends_fix() {
        let (temp_dir, control_path) = setup_control(
            "Source: test-pkg\n\n\
             Package: test-pkg\nArchitecture: all\nDepends: perl-modules-5.28\n",
        );

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated,
            "Source: test-pkg\n\n\
             Package: test-pkg\nArchitecture: all\nDepends: perl\n",
        );
    }

    #[test]
    fn test_skips_perl_source_package() {
        let (temp_dir, _) = setup_control(
            "Source: perl\nBuild-Depends: perl-modules\n\n\
             Package: perl-base\nArchitecture: any\n",
        );

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_changes_when_no_perl_modules() {
        let (temp_dir, _) = setup_control(
            "Source: test-pkg\nBuild-Depends: debhelper-compat (= 13)\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_replaces_field_not_checked() {
        let input = "Source: test-pkg\n\n\
             Package: test-pkg\nArchitecture: all\nReplaces: perl-modules\n";
        let (temp_dir, control_path) = setup_control(input);

        let result = run(temp_dir.path());
        assert!(matches!(result, Err(FixerError::NoChanges)));

        let updated = fs::read_to_string(&control_path).unwrap();
        assert_eq!(updated, input);
    }

    #[test]
    fn test_dedup_perl_after_replacement() {
        let (temp_dir, control_path) = setup_control(
            "Source: test-pkg\nBuild-Depends: perl, perl-modules\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );

        let result = run(temp_dir.path());
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

        let updated = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            updated,
            "Source: test-pkg\nBuild-Depends: perl\n\n\
             Package: test-pkg\nArchitecture: all\n",
        );
    }
}
