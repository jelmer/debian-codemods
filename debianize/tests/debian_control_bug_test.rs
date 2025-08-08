use debian_control::lossless::Control;

#[test]
fn test_testsuite_field_newline_bug() {
    // Create a control object directly
    let mut control = Control::new();

    let mut source = control.add_source("test-package");
    source.set_maintainer("Test Maintainer <test@example.com>");
    source.set_standards_version("4.7.0");
    source.set_testsuite("autopkgtest-pkg-python");

    // Add fields that should come after Testsuite
    source
        .as_mut_deb822()
        .insert("Vcs-Git", "https://example.com/repo.git");
    source
        .as_mut_deb822()
        .insert("Vcs-Browser", "https://example.com/repo");

    // Serialize to string
    let content = control.to_string();
    println!("Generated content:");
    println!("{:?}", content);

    // Check if there's a newline after Testsuite
    assert!(
        !content.contains("autopkgtest-pkg-pythonVcs-"),
        "BUG: No newline after Testsuite field! Content: {}",
        content
    );

    // The content should have proper newlines
    assert!(
        content.contains("Testsuite: autopkgtest-pkg-python\n"),
        "Testsuite field should end with newline"
    );
}

#[test]
fn test_field_ordering_and_newlines() {
    let mut control = Control::new();
    let mut source = control.add_source("python-test");

    // Set fields in the order we expect them
    source.set_maintainer("John Doe <john@example.com>");
    source.set_rules_requires_root(false);
    source.set_standards_version("4.7.0");
    source.set_build_depends(&"debhelper-compat (= 13), python3-all".parse().unwrap());
    source.set_testsuite("autopkgtest-pkg-python");

    // Add VCS fields using raw deb822
    source
        .as_mut_deb822()
        .insert("Vcs-Git", "https://example.com/repo.git");
    source
        .as_mut_deb822()
        .insert("Vcs-Browser", "https://example.com/repo");

    let content = control.to_string();

    // Debug output
    println!("\nGenerated control file:");
    println!("{}", content);
    println!("\nAs debug string: {:?}", content);

    // Check each field is on its own line
    let lines: Vec<&str> = content.lines().collect();

    // Find the Testsuite line
    let testsuite_line_idx = lines
        .iter()
        .position(|&line| line.starts_with("Testsuite:"));
    assert!(testsuite_line_idx.is_some(), "Should have Testsuite field");

    if let Some(idx) = testsuite_line_idx {
        println!("Testsuite is at line {}: '{}'", idx, lines[idx]);

        // Check the next line
        if idx + 1 < lines.len() {
            let next_line = lines[idx + 1];
            println!("Next line after Testsuite: '{}'", next_line);

            // The next line should either be empty or start with a field name
            assert!(
                next_line.is_empty() || next_line.contains(":"),
                "Line after Testsuite should be empty or a new field, got: '{}'",
                next_line
            );
        }
    }
}
