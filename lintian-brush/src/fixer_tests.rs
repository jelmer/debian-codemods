use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/fixer_tests.rs"));

/// Check if two lists of LintianIssues match, supporting wildcards (*) in expected info fields
fn issues_match_with_wildcards(expected: &[LintianIssue], actual: &[LintianIssue]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    for (exp, act) in expected.iter().zip(actual.iter()) {
        // Check package, package_type, and tag match exactly
        if exp.package != act.package || exp.package_type != act.package_type || exp.tag != act.tag
        {
            return false;
        }

        // Check info field with wildcard support
        match (&exp.info, &act.info) {
            (Some(exp_info), Some(act_info)) => {
                if !crate::lintian_overrides::info_matches(exp_info, act_info) {
                    return false;
                }
            }
            (None, None) => {}
            _ => return false,
        }
    }

    true
}

// Verifying that every test directory maps to a fixer is only meaningful when
// all feature-gated fixers are built; a lean build legitimately omits some.
#[cfg(all(feature = "pgp", feature = "bts", feature = "upstream"))]
#[test]
fn test_all_test_dirs_have_matching_fixers() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tests_dir = Path::new(manifest_dir).join("tests");

    // Get list of all fixer names from all_lintian_fixers() (including disabled ones)
    let all_fixers = all_lintian_fixers();

    let fixer_names: std::collections::HashSet<String> =
        all_fixers.map(|f| f.name().to_string()).collect();

    // Get list of test directories
    let test_dirs = std::fs::read_dir(&tests_dir).expect("Failed to read tests directory");

    let mut tests_without_fixers = Vec::new();

    for entry in test_dirs {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.is_dir() {
            let test_name = entry.file_name().to_string_lossy().to_string();

            // Skip README.md and slow directory
            if test_name == "slow" || test_name.starts_with('.') {
                continue;
            }

            // Check if there's a matching fixer
            if !fixer_names.contains(&test_name) {
                tests_without_fixers.push(test_name);
            }
        }
    }

    if !tests_without_fixers.is_empty() {
        panic!(
            "The following test directories have no matching fixers in all_lintian_fixers():\n{}",
            tests_without_fixers.join("\n")
        );
    }
}

#[test]
fn test_all_fixers_handle_missing_source_stanza() {
    let fixers = all_lintian_fixers();

    let mut failures = Vec::new();

    for fixer in fixers {
        let td = tempfile::tempdir().unwrap();
        let testdir = td.path().join("testdir");
        std::fs::create_dir_all(testdir.join("debian")).unwrap();

        // Write a control file with only a binary stanza, no Source stanza
        std::fs::write(
            testdir.join("debian/control"),
            "Package: test-binary\nArchitecture: any\nDescription: A test package\n",
        )
        .unwrap();

        // Write a minimal changelog so version parsing works
        std::fs::write(
            testdir.join("debian/changelog"),
            "test-package (1.0-1) unstable; urgency=low\n\n  * Initial release.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            compat_release: Some("sid".to_string()),
            minimum_certainty: None,
            net_access: Some(false),
            trust_package: Some(false),
            opinionated: Some(false),
            ..Default::default()
        };

        let current_version: debversion::Version = "1.0-1".parse().unwrap();

        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            &testdir,
            Some("test-package".into()),
            Some(current_version.clone()),
        );

        let result = crate::detector::detect_and_fix(fixer.as_ref(), &ws, &preferences);

        match result {
            Ok(_) => {}                      // Fixer made changes, that's fine
            Err(FixerError::NoChanges) => {} // Expected for most fixers
            Err(FixerError::NoChangesAfterOverrides(_)) => {}
            Err(FixerError::NotDebianPackage(_)) => {}
            Err(FixerError::NotCertainEnough(..)) => {}
            Err(FixerError::FormattingUnpreservable(_)) => {}
            Err(FixerError::GeneratedFile(_)) => {}
            Err(FixerError::Other(_)) => {} // Non-fatal errors are acceptable
            Err(FixerError::Io(_)) => {}    // Missing other files is not what we're testing
            Err(FixerError::ScriptNotFound(_)) => {} // Missing scripts is not what we're testing
            Err(e) => {
                failures.push(format!("Fixer '{}' crashed: {:?}", fixer.name(), e));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "The following fixers crashed on a control file without a Source stanza:\n{}",
            failures.join("\n")
        );
    }
}

/// Recursively compare two directory trees, returning a list of human-
/// readable differences (empty when the trees match).
///
/// Mirrors the `diff --no-dereference -x '*~' -ur` invocation this used to
/// shell out to: backup files (`*~`) are ignored, symlinks are compared by
/// their target rather than dereferenced, and regular files are compared by
/// content. Doing the comparison in-process avoids a fork/exec of `diff`,
/// which raced under heavy test parallelism.
fn compare_trees(expected: &Path, actual: &Path) -> Vec<String> {
    let mut differences = Vec::new();
    compare_dir(expected, actual, Path::new(""), &mut differences);
    differences
}

/// Sorted names of the entries in `dir`, excluding `*~` backup files.
/// Returns an empty list when `dir` doesn't exist.
fn tree_entries(dir: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<std::ffi::OsString> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.expect("Failed to read directory entry").file_name())
            .filter(|name| !name.to_string_lossy().ends_with('~'))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("Error reading {}: {}", dir.display(), e),
    };
    names.sort();
    names
}

/// Compare the directory `expected/rel` against `actual/rel`, appending any
/// differences (described relative to the tree roots) to `differences`.
fn compare_dir(expected: &Path, actual: &Path, rel: &Path, differences: &mut Vec<String>) {
    let expected_names = tree_entries(&expected.join(rel));
    let actual_names = tree_entries(&actual.join(rel));

    let mut all: Vec<&std::ffi::OsString> =
        expected_names.iter().chain(actual_names.iter()).collect();
    all.sort();
    all.dedup();

    for name in all {
        let child = rel.join(name);
        let in_expected = expected_names.contains(name);
        let in_actual = actual_names.contains(name);
        match (in_expected, in_actual) {
            (true, false) => differences.push(format!("Only in expected: {}", child.display())),
            (false, true) => differences.push(format!("Only in actual: {}", child.display())),
            (true, true) => compare_entry(expected, actual, &child, differences),
            (false, false) => unreachable!(),
        }
    }
}

/// Compare the single entry at `rel` between the two trees.
fn compare_entry(expected: &Path, actual: &Path, rel: &Path, differences: &mut Vec<String>) {
    let expected_path = expected.join(rel);
    let actual_path = actual.join(rel);
    // `symlink_metadata` does not follow symlinks (matches `--no-dereference`).
    let expected_meta = std::fs::symlink_metadata(&expected_path)
        .unwrap_or_else(|e| panic!("Error stat-ing {}: {}", expected_path.display(), e));
    let actual_meta = std::fs::symlink_metadata(&actual_path)
        .unwrap_or_else(|e| panic!("Error stat-ing {}: {}", actual_path.display(), e));

    let expected_type = expected_meta.file_type();
    let actual_type = actual_meta.file_type();

    if expected_type.is_symlink() || actual_type.is_symlink() {
        if !expected_type.is_symlink() || !actual_type.is_symlink() {
            differences.push(format!(
                "{}: symlink vs non-symlink mismatch",
                rel.display()
            ));
            return;
        }
        let expected_target = std::fs::read_link(&expected_path).unwrap();
        let actual_target = std::fs::read_link(&actual_path).unwrap();
        if expected_target != actual_target {
            differences.push(format!(
                "{}: symlink target differs ({} vs {})",
                rel.display(),
                expected_target.display(),
                actual_target.display()
            ));
        }
        return;
    }

    if expected_type.is_dir() != actual_type.is_dir() {
        differences.push(format!("{}: directory vs file mismatch", rel.display()));
        return;
    }

    if expected_type.is_dir() {
        compare_dir(expected, actual, rel, differences);
        return;
    }

    let expected_bytes = std::fs::read(&expected_path).unwrap();
    let actual_bytes = std::fs::read(&actual_path).unwrap();
    if expected_bytes != actual_bytes {
        differences.push(format!(
            "{}: content differs\n--- expected\n{}\n--- actual\n{}",
            rel.display(),
            String::from_utf8_lossy(&expected_bytes),
            String::from_utf8_lossy(&actual_bytes),
        ));
    }
}

fn run_fixer_testcase(fixer_name: &str, test_name: &str, path: &Path) {
    let td = tempfile::tempdir().unwrap();

    let indir = path.join("in");
    let outdir = path.join("out");

    let testdir = td.path().join("testdir");
    std::fs::create_dir(&testdir).unwrap();

    // recursively copy indir to td/in
    let mut options = fs_extra::dir::CopyOptions::new();
    options.copy_inside = true;
    options.content_only = true;
    fs_extra::dir::copy(indir, &testdir, &options).unwrap();

    let xfail_path = path.join("xfail");
    match std::fs::read_to_string(&xfail_path) {
        Ok(s) => {
            eprintln!(
                "Skipping test {} because it is expected to fail: {}",
                test_name, s
            );
            return;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("Error reading {}: {}", xfail_path.display(), e),
    }

    // Parse env file to configure preferences and check for version override
    // Match Python defaults from py/lintian_brush/fixer.py
    let mut preferences = FixerPreferences {
        compat_release: Some("sid".to_string()),
        minimum_certainty: None, // Python default is None when MINIMUM_CERTAINTY is not set
        net_access: Some(false), // NET_ACCESS defaults to "disallow"
        trust_package: Some(false), // TRUST_PACKAGE defaults to false unless explicitly "true"
        opinionated: Some(false), // OPINIONATED defaults to "no"
        ..Default::default()
    };
    let mut current_version_override = None;
    let mut extra_env = std::collections::HashMap::new();

    let env_path = path.join("env");
    match std::fs::File::open(&env_path) {
        Ok(f) => {
            use std::io::BufRead;
            let br = std::io::BufReader::new(f);
            for line in br.lines() {
                let line = line.unwrap();
                if let Some((name, value)) = line.split_once('=') {
                    match name {
                        "MINIMUM_CERTAINTY" => {
                            preferences.minimum_certainty = Some(match value {
                                "certain" => Certainty::Certain,
                                "confident" => Certainty::Confident,
                                "likely" => Certainty::Likely,
                                "possible" => Certainty::Possible,
                                _ => panic!("Unknown certainty value: {}", value),
                            });
                        }
                        "COMPAT_RELEASE" => {
                            preferences.compat_release = Some(value.to_string());
                        }
                        "UPGRADE_RELEASE" => {
                            preferences.upgrade_release = Some(value.to_string());
                        }
                        "OPINIONATED" => {
                            preferences.opinionated = Some(value == "yes");
                        }
                        "NET_ACCESS" => {
                            preferences.net_access = Some(match value {
                                "allow" => true,
                                "disallow" => false,
                                _ => panic!(
                                    "Unknown NET_ACCESS value: {} (must be 'allow' or 'disallow')",
                                    value
                                ),
                            });
                        }
                        "TRUST_PACKAGE" => {
                            preferences.trust_package = Some(match value {
                                "true" => true,
                                "false" => false,
                                _ => panic!(
                                    "Unknown TRUST_PACKAGE value: {} (must be 'true' or 'false')",
                                    value
                                ),
                            });
                        }
                        "CURRENT_VERSION" => {
                            current_version_override = Some(value.parse().unwrap());
                        }
                        _ => {
                            // Pass through any other environment variables to the fixer
                            extra_env.insert(name.to_string(), value.to_string());
                        }
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("Error reading {}: {}", env_path.display(), e),
    }

    // Set extra environment variables if any were found
    if !extra_env.is_empty() {
        preferences.extra_env = Some(extra_env);
    }

    // Determine current version - either from override or from changelog
    let cl_path = testdir.join("debian/changelog");
    let current_version = if let Some(version) = current_version_override {
        version
    } else {
        match std::fs::File::open(&cl_path) {
            Ok(f) => {
                match ChangeLog::read_relaxed(f) {
                    Ok(cl) => {
                        let first_entry = cl.iter().next().unwrap();
                        let version = first_entry.version().unwrap();
                        if first_entry.distributions().as_deref().unwrap() == vec!["UNRELEASED"] {
                            version
                        } else {
                            let mut version = version;
                            version.increment_debian();
                            version
                        }
                    }
                    Err(_) => {
                        // If changelog parsing fails (e.g., due to malformed content that the fixer is meant to fix),
                        // use a default version
                        "1.0-1".parse().unwrap()
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "1.0-1".parse().unwrap(),
            Err(e) => panic!("Error reading {}: {}", cl_path.display(), e),
        }
    };

    // Use the regular fixer infrastructure to find and run the fixer
    // Force subprocess mode for all fixers to avoid Python GIL race conditions in parallel tests
    let fixers = all_lintian_fixers();
    let Some(fixer) = fixers.into_iter().find(|f| f.name() == fixer_name) else {
        // The fixer is excluded from this build by a feature flag; its
        // test directory still exists but there is nothing to exercise.
        eprintln!(
            "Skipping test {}: fixer '{}' is not built",
            test_name, fixer_name
        );
        return;
    };

    let ws = debian_workspace::fs_workspace::FsWorkspace::new(
        &testdir,
        Some("test-package".into()),
        Some(current_version.clone()),
    );
    let (actual_result, exit_code) =
        match crate::detector::detect_and_fix(fixer.as_ref(), &ws, &preferences) {
            Ok(result) => (Some(result), 0),
            Err(FixerError::NoChanges) => {
                eprintln!("Fixer returned NoChanges for test {}", test_name);
                (None, 1) // Exit code 1 for no changes
            }
            Err(FixerError::NoChangesAfterOverrides(_)) => {
                eprintln!(
                    "Fixer returned NoChangesAfterOverrides for test {}",
                    test_name
                );
                (None, 1) // Exit code 1 for no changes
            }
            Err(e) => {
                if let FixerError::ScriptFailed {
                    path,
                    exit_code,
                    stderr,
                } = &e
                {
                    eprintln!(
                        "Script failed: {} (exit code: {})",
                        path.display(),
                        exit_code
                    );
                    if !stderr.is_empty() {
                        eprintln!("Stderr:\n{}", stderr);
                    }
                }
                panic!("Fixer error: {:?}", e);
            }
        };

    if exit_code != 0 && exit_code != 1 {
        panic!("Test {} failed with exit code {}", test_name, exit_code);
    }

    // Only check the result tree if we expect changes (exit_code == 0).
    if exit_code == 0 {
        let expected_dir = if outdir.is_symlink() {
            path.join(std::fs::read_link(&outdir).unwrap())
        } else {
            outdir.clone()
        };
        let differences = compare_trees(&expected_dir, &testdir);
        if !differences.is_empty() {
            eprintln!("Differences:\n{}", differences.join("\n"));
            panic!("Test {} failed", test_name);
        }
    }

    let check_message = !outdir.is_symlink() || outdir.read_link().unwrap() != PathBuf::from("in");

    let message_path = path.join("message");
    match std::fs::read_to_string(&message_path) {
        Ok(expected_message) => {
            // Parse both the expected and actual output as FixerResult
            let expected_result = match parse_script_fixer_output(&expected_message) {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Failed to parse expected message as FixerResult: {:?}", e);
                    eprintln!("Expected message:\n{}", expected_message);
                    panic!(
                        "Test {} failed - invalid expected message format",
                        test_name
                    );
                }
            };

            // Get the actual result from the fixer run
            let actual_result = actual_result
                .as_ref()
                .expect("Expected a FixerResult but fixer returned NoChanges");

            // Compare the parsed results
            if expected_result.description != actual_result.description {
                eprintln!("Expected description: {:?}", expected_result.description);
                eprintln!("Got description: {:?}", actual_result.description);
                panic!("Test {} failed - description mismatch", test_name);
            }

            let expected_tags: HashSet<&str> =
                expected_result.fixed_lintian_tags().into_iter().collect();
            let actual_tags: HashSet<&str> =
                actual_result.fixed_lintian_tags().into_iter().collect();
            if expected_tags != actual_tags {
                eprintln!("Expected tags: {:?}", expected_tags);
                eprintln!("Got tags: {:?}", actual_tags);
                panic!("Test {} failed - tags mismatch", test_name);
            }

            // Compare full issue details including info field, supporting wildcards
            if !issues_match_with_wildcards(
                &expected_result.fixed_lintian_issues,
                &actual_result.fixed_lintian_issues,
            ) {
                eprintln!(
                    "Expected issues: {:?}",
                    expected_result.fixed_lintian_issues
                );
                eprintln!("Got issues: {:?}", actual_result.fixed_lintian_issues);
                eprintln!("\nExpected message format:");
                eprintln!("Fixed-Lintian-Issues:");
                for issue in &actual_result.fixed_lintian_issues {
                    eprintln!(" {}", issue);
                }
                panic!("Test {} failed - issue details mismatch", test_name);
            }

            if expected_result.certainty != actual_result.certainty {
                eprintln!("Expected certainty: {:?}", expected_result.certainty);
                eprintln!("Got certainty: {:?}", actual_result.certainty);
                panic!("Test {} failed - certainty mismatch", test_name);
            }

            if expected_result.patch_name != actual_result.patch_name {
                eprintln!("Expected patch_name: {:?}", expected_result.patch_name);
                eprintln!("Got patch_name: {:?}", actual_result.patch_name);
                panic!("Test {} failed - patch_name mismatch", test_name);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if check_message {
                panic!("No message file found for test {}", test_name);
            }
        }
        Err(e) => panic!("Error reading {}: {}", message_path.display(), e),
    }
}
