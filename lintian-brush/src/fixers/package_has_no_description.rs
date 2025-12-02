use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

fn textwrap_description(text: &str) -> Vec<String> {
    let mut ret = Vec::new();
    let text = text.trim();
    let paras: Vec<&str> = text.split("\n\n").collect();

    for (i, para) in paras.iter().enumerate() {
        if para.contains("\n*") {
            ret.extend(para.lines().map(|s| s.to_string()));
        } else {
            // Use textwrap to wrap at default width (70 chars to match Python)
            // Use word_separator to split only on whitespace (like Python's textwrap)
            let options =
                textwrap::Options::new(70).word_separator(textwrap::WordSeparator::AsciiSpace);
            let wrapped = textwrap::wrap(para, options);
            ret.extend(wrapped.into_iter().map(|s| s.to_string()));
        }
        // Add empty line between paragraphs, but not after the last one
        if i < paras.len() - 1 {
            ret.push(String::new());
        }
    }

    ret
}

fn format_description(summary: &str, lines: &[String]) -> String {
    let mut result = summary.to_string();

    for line in lines {
        result.push('\n');
        // Don't add leading space - deb822-lossless handles indentation via Entry::with_formatting
        if line.is_empty() {
            result.push('.');
        } else {
            result.push_str(line);
        }
    }

    result
}

fn guess_description(
    base_path: &Path,
    _binary_name: &str,
    binary_count: usize,
    summary: Option<&str>,
    preferences: &FixerPreferences,
) -> Option<String> {
    if binary_count != 1 {
        // TODO: Support handling multiple binaries
        return None;
    }

    // Create a tokio runtime to call the async function
    let rt = tokio::runtime::Runtime::new().ok()?;

    let trust_package = if preferences.trust_package.unwrap_or(false) {
        Some(true)
    } else {
        None
    };

    let net_access = preferences.net_access;

    rt.block_on(async {
        // TODO: This upstream_ontologist usage should be shared with no-homepage-field fixer.
        // Consider extracting a common helper function for calling guess_upstream_metadata
        // with proper parameters.
        let metadata = upstream_ontologist::guess_upstream_metadata(
            base_path,
            trust_package,
            net_access,
            None, // consult_external_directory
            None, // check
        )
        .await
        .ok()?;

        let summary = summary.or_else(|| {
            metadata.get("Summary").and_then(|s| {
                if let upstream_ontologist::UpstreamDatum::Summary(summary_text) = &s.datum {
                    Some(summary_text.as_str())
                } else {
                    None
                }
            })
        });

        // Get description
        if let Some(desc_datum) = metadata.get("Description") {
            if let upstream_ontologist::UpstreamDatum::Description(desc_text) = &desc_datum.datum {
                let upstream_description = textwrap_description(desc_text);

                if let Some(summary) = summary {
                    let lines: Vec<String> = upstream_description
                        .into_iter()
                        .map(|line| {
                            if line.is_empty() {
                                ".".to_string()
                            } else {
                                line
                            }
                        })
                        .collect();

                    return Some(format_description(summary, &lines));
                } else if upstream_description.len() == 1 {
                    return Some(upstream_description[0].trim_end_matches('\n').to_string());
                }
            }
        }

        // Better than nothing - just return the summary if we have it
        summary.map(|s| s.to_string())
    })
}

pub fn run(
    base_path: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut updated = Vec::new();
    let mut fixed_tags = Vec::new();

    // Count binaries first
    let binary_count = editor.binaries().count();

    // Now iterate and modify
    for mut binary in editor.binaries() {
        let package_name = binary.name().unwrap_or_default();

        let existing_description = binary.description().unwrap_or_default();

        let (summary, tag) = if existing_description.is_empty() {
            (None, "required-field")
        } else if existing_description.trim().lines().count() == 1 {
            (
                Some(existing_description.lines().next().unwrap_or("")),
                "extended-description-is-empty",
            )
        } else {
            continue;
        };

        if let Some(description) =
            guess_description(base_path, &package_name, binary_count, summary, preferences)
        {
            if description != existing_description {
                binary.set_description(Some(&description));
                updated.push(package_name.clone());
                fixed_tags.push((tag, package_name));
            }
        }
    }

    if updated.is_empty() {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    updated.sort();
    let description = format!(
        "Add description for binary packages: {}",
        updated.join(", ")
    );

    let mut result = FixerResult::builder(description).certainty(crate::Certainty::Possible);

    for (tag, _package) in fixed_tags {
        result = result.fixed_tag(tag);
    }

    Ok(result.build())
}

declare_fixer! {
    name: "package-has-no-description",
    tags: ["required-field", "extended-description-is-empty"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_changes_when_description_exists() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        let control_content = r#"Source: test-package
Maintainer: Test User <test@example.com>

Package: test-package
Description: Test package
 This is a test package with a proper description.
"#;
        fs::write(&control_path, control_content).unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, "test-package", &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_textwrap_description() {
        let text = "This is a long line that should be wrapped at around 79 characters to fit properly in the description field.";
        let lines = textwrap_description(text);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.len() <= 79));
    }

    #[test]
    fn test_textwrap_description_with_paragraphs() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let lines = textwrap_description(text);
        assert_eq!(lines.len(), 3); // First para, empty line, second para
        assert_eq!(lines[0], "First paragraph.");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "Second paragraph.");
    }

    #[test]
    fn test_textwrap_description_with_bullets() {
        let text = "Features:\n* Feature one\n* Feature two";
        let lines = textwrap_description(text);
        assert!(lines.contains(&"Features:".to_string()));
        assert!(lines.contains(&"* Feature one".to_string()));
        assert!(lines.contains(&"* Feature two".to_string()));
    }

    #[test]
    fn test_textwrap_description_very_long_line() {
        let text = "A ".repeat(100); // Create a very long line
        let lines = textwrap_description(&text);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.len() <= 79));
    }

    #[test]
    fn test_format_description() {
        let summary = "Short summary";
        let lines = vec![
            "First line".to_string(),
            "".to_string(),
            "Third line".to_string(),
        ];
        let result = format_description(summary, &lines);
        assert_eq!(result, "Short summary\nFirst line\n.\nThird line");
    }

    #[test]
    fn test_textwrap_exact_readme_case() {
        // This is the exact text from the readme test
        let text = "BLAH is a C++ wrapper library around [Example](https://ww.example.com/) with the aim of supporting ISO 191007:2013 and OGC Simple Features for 3D operations.\n\nAnd here is some more information about it.";
        let lines = textwrap_description(text);

        // Expected from Python textwrap.wrap()
        let expected = vec![
            "BLAH is a C++ wrapper library around",
            "[Example](https://ww.example.com/) with the aim of supporting ISO",
            "191007:2013 and OGC Simple Features for 3D operations.",
            "",
            "And here is some more information about it.",
        ];

        assert_eq!(lines, expected);
    }
}
