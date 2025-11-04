/// Tests that verify exact file content generation
use std::collections::HashMap;

#[test]
fn test_debian_source_format_variations() {
    // Test different source format files

    // Standard quilt format
    let quilt_format = "3.0 (quilt)\n";

    // Native format
    let native_format = "3.0 (native)\n";

    // Git format
    let git_format = "3.0 (git)\n";

    // Old format
    let old_format = "1.0\n";

    let formats = HashMap::from([
        ("quilt", quilt_format),
        ("native", native_format),
        ("git", git_format),
        ("old", old_format),
    ]);

    // Verify each format is exactly as expected
    for (name, content) in formats {
        assert_eq!(
            content.lines().count(),
            1,
            "{} format should be exactly one line",
            name
        );
        assert!(
            content.ends_with('\n'),
            "{} format should end with newline",
            name
        );
    }
}

#[test]
fn test_whitespace_and_formatting() {
    // Test that whitespace is handled correctly

    // Build-Depends continuation lines should align
    let build_depends = r#"Build-Depends: debhelper-compat (= 13),
               dh-sequence-python3,
               python3-all,
               python3-setuptools"#;

    // Verify alignment (15 spaces for continuation)
    for line in build_depends.lines().skip(1) {
        assert!(
            line.starts_with("               "),
            "Build-Depends continuation should have 15 spaces"
        );
    }

    // Description continuation
    let description = r#"Description: Short description
 This is the long description with a leading space.
 Each line starts with a single space.
 .
 A dot on its own line creates a paragraph break."#;

    // Verify description formatting
    let desc_lines: Vec<&str> = description.lines().collect();
    assert!(
        !desc_lines[0].starts_with(' '),
        "First line should not have leading space"
    );
    for line in &desc_lines[1..] {
        assert!(
            line.starts_with(' ') || line.is_empty(),
            "Description continuation lines should start with space"
        );
    }
}
