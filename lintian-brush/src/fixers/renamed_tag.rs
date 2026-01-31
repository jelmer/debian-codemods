use crate::lintian_overrides::{copy_node, AstNode, LintianOverrides, OverrideLine, SyntaxKind};
use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use rowan::{GreenNodeBuilder, SyntaxNode};
use std::path::Path;

// Include the generated renamed tags map
include!(concat!(env!("OUT_DIR"), "/renamed_tags.rs"));

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Load the renamed tags mapping from the compiled data
    let renames = get_renamed_tags();

    // Find override files to process
    let override_files = find_override_files(base_path);

    if override_files.is_empty() {
        return Err(FixerError::NoChanges);
    }

    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    for override_file in override_files {
        let content = std::fs::read_to_string(&override_file).map_err(|e| {
            FixerError::Other(format!("Failed to read {}: {}", override_file.display(), e))
        })?;

        let parsed = LintianOverrides::parse(&content);
        if !parsed.errors().is_empty() {
            // Skip files with parse errors
            continue;
        }

        let overrides = parsed.ok().unwrap();

        let (updated_overrides, file_fixed, file_overridden) =
            update_renamed_tags(&overrides, &renames, base_path);

        fixed_issues.extend(file_fixed);
        overridden_issues.extend(file_overridden);

        if let Some(new_overrides) = updated_overrides {
            let new_content = new_overrides.text();
            std::fs::write(&override_file, new_content).map_err(|e| {
                FixerError::Other(format!(
                    "Failed to write {}: {}",
                    override_file.display(),
                    e
                ))
            })?;
        }
    }

    if fixed_issues.is_empty() {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    Ok(
        FixerResult::builder("Update renamed lintian tag names in lintian overrides.")
            .fixed_issues(fixed_issues)
            .overridden_issues(overridden_issues)
            .build(),
    )
}

fn find_override_files(base_path: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Check debian/source/lintian-overrides
    let source_overrides = base_path.join("debian/source/lintian-overrides");
    if source_overrides.exists() {
        files.push(source_overrides);
    }

    // Check debian/*.lintian-overrides
    let debian_dir = base_path.join("debian");
    if debian_dir.exists() && debian_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&debian_dir) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".lintian-overrides") {
                        files.push(entry.path());
                    }
                }
            }
        }
    }

    files
}

/// Update renamed tags in a LintianOverrides tree
// TODO: Move AST manipulation logic to lintian_overrides.rs as a generic helper function
fn update_renamed_tags(
    overrides: &LintianOverrides,
    renames: &indexmap::IndexMap<&str, &str>,
    base_path: &Path,
) -> (
    Option<LintianOverrides>,
    Vec<LintianIssue>,
    Vec<LintianIssue>,
) {
    let mut builder = GreenNodeBuilder::new();
    let mut changed = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    builder.start_node(SyntaxKind::ROOT.into());

    for child in overrides.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(node) if node.kind() == SyntaxKind::OVERRIDE_LINE => {
                let line = OverrideLine::cast(node.clone()).unwrap();

                // Check if this line has a tag that needs renaming
                if let Some(tag_token) = line.tag() {
                    let tag_text = tag_token.text();
                    if let Some(new_tag) = renames.get(tag_text) {
                        let issue = LintianIssue::source_with_info(
                            "renamed-tag",
                            vec![format!("{} => {}", tag_text, new_tag)],
                        );

                        if !issue.should_fix(base_path) {
                            overridden_issues.push(issue);
                            // Copy the line as-is since it's overridden
                            copy_node(&mut builder, &node);
                        } else {
                            // Rebuild this line with the new tag
                            builder.start_node(SyntaxKind::OVERRIDE_LINE.into());

                            for element in line.syntax().children_with_tokens() {
                                match element {
                                    rowan::NodeOrToken::Token(token)
                                        if token.kind() == SyntaxKind::TAG =>
                                    {
                                        builder.token(SyntaxKind::TAG.into(), new_tag);
                                        changed = true;
                                    }
                                    rowan::NodeOrToken::Token(token) => {
                                        builder.token(token.kind().into(), token.text());
                                    }
                                    rowan::NodeOrToken::Node(child_node) => {
                                        // Recursively copy nodes (e.g., PACKAGE_SPEC)
                                        copy_node(&mut builder, &child_node);
                                    }
                                }
                            }

                            builder.finish_node();
                            fixed_issues.push(issue);
                        }
                    } else {
                        // Copy the line as-is
                        copy_node(&mut builder, &node);
                    }
                } else {
                    // No tag, copy as-is
                    copy_node(&mut builder, &node);
                }
            }
            rowan::NodeOrToken::Node(node) => {
                copy_node(&mut builder, &node);
            }
            rowan::NodeOrToken::Token(token) => {
                builder.token(token.kind().into(), token.text());
            }
        }
    }

    builder.finish_node();

    let updated_overrides = if changed {
        let green = builder.finish();
        let syntax = SyntaxNode::<crate::lintian_overrides::Lang>::new_root(green);
        LintianOverrides::cast(syntax)
    } else {
        None
    };

    (updated_overrides, fixed_issues, overridden_issues)
}

declare_fixer! {
    name: "renamed-tag",
    tags: ["renamed-tag"],
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
    fn test_no_override_files() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_renames_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let override_path = debian_dir.join("lintian-overrides");
        fs::write(
            &override_path,
            "# Comment line\nsource-package-name: some-current-tag some info\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify file content unchanged
        let content = fs::read_to_string(&override_path).unwrap();
        assert_eq!(
            content,
            "# Comment line\nsource-package-name: some-current-tag some info\n"
        );
    }

    #[test]
    fn test_rename_tags() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let override_path = debian_dir.join("test-package.lintian-overrides");
        let content = "# Comment\nsource-package: debian-changelog-has-wrong-weekday some info\nbinary-package: binary-without-manpage\n";
        fs::write(&override_path, content).unwrap();

        // First test parsing
        let parsed = LintianOverrides::parse(content);
        assert!(
            parsed.errors().is_empty(),
            "Parse errors: {:?}",
            parsed.errors()
        );
        let overrides = parsed.ok().unwrap();

        // Check that tags are found
        let lines: Vec<_> = overrides.lines().collect();
        assert_eq!(lines.len(), 3, "Expected 3 lines"); // Comment, two overrides

        let tags: Vec<_> = lines.iter().filter_map(|l| l.tag()).collect();
        assert_eq!(tags.len(), 2, "Expected 2 tags");
        assert_eq!(tags[0].text(), "debian-changelog-has-wrong-weekday");
        assert_eq!(tags[1].text(), "binary-without-manpage");

        let result = run(base_path);
        assert!(result.is_ok(), "Result failed: {:?}", result);

        let updated_content = fs::read_to_string(&override_path).unwrap();
        assert_eq!(
            updated_content,
            "# Comment\nsource-package: debian-changelog-has-wrong-day-of-week some info\nbinary-package: no-manual-page\n"
        );
    }

    #[test]
    fn test_source_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_source_dir = base_path.join("debian/source");
        fs::create_dir_all(&debian_source_dir).unwrap();

        let override_path = debian_source_dir.join("lintian-overrides");
        fs::write(&override_path, "debian-changelog-has-wrong-weekday\n").unwrap();

        let result = run(base_path);
        assert!(result.is_ok(), "Result failed: {:?}", result);

        let updated_content = fs::read_to_string(&override_path).unwrap();
        assert_eq!(updated_content, "debian-changelog-has-wrong-day-of-week\n");
    }

    #[test]
    fn test_update_renamed_tags_function() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let text = "old-tag some info\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();

        let mut renames = indexmap::IndexMap::new();
        renames.insert("old-tag", "new-tag");

        let (updated, fixed, overridden) = update_renamed_tags(&overrides, &renames, base_path);

        assert!(updated.is_some());
        let result = updated.unwrap().text();

        assert!(result.contains("new-tag"));
        assert!(!result.contains("old-tag"));
        assert!(result.contains("some info"));

        assert_eq!(fixed.len(), 1);
        assert_eq!(overridden.len(), 0);
    }
}
