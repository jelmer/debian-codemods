use crate::{declare_fixer, FixerError, FixerResult, LintianIssue};
use debian_analyzer::rules::check_cdbs;
use debian_control::Control;
use makefile_lossless::Makefile;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn get_archs(base_path: &Path) -> Result<HashSet<String>, FixerError> {
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Ok(HashSet::new());
    }

    let content = fs::read_to_string(&control_path)?;
    let control = Control::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse control file: {}", e)))?;

    let mut archs = HashSet::new();
    for binary in control.binaries() {
        if let Some(arch) = binary.architecture() {
            archs.insert(arch.to_string());
        }
    }

    Ok(archs)
}

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let rules_path = base_path.join("debian/rules");

    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let mut makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // Check if build-arch and build-indep targets already exist or are matched by wildcards
    let has_build_arch = makefile.find_rule_by_target_pattern("build-arch").is_some();
    let has_build_indep = makefile
        .find_rule_by_target_pattern("build-indep")
        .is_some();

    if has_build_arch && has_build_indep {
        return Err(FixerError::NoChanges);
    }

    // Check for includes - we don't handle those yet
    // check_cdbs also checks for other includes
    if check_cdbs(&rules_path) || makefile.includes().count() > 0 {
        return Err(FixerError::NoChanges);
    }

    let archs = get_archs(base_path)?;
    let mut added = Vec::new();
    let mut fixed_issues = Vec::new();

    // Add build-indep if missing
    if !has_build_indep {
        let issue = LintianIssue::source_with_info(
            "debian-rules-missing-recommended-target",
            vec!["build-indep [debian/rules]".to_string()],
        );
        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        added.push("build-indep");
        fixed_issues.push(issue);
        let mut rule = makefile.add_rule("build-indep");

        // If architecture is "all", make it depend on "build"
        if archs.contains("all") {
            rule.add_prerequisite("build")
                .map_err(|e| FixerError::Other(format!("Failed to add prerequisite: {}", e)))?;
        }
    }

    // Add build-arch if missing
    if !has_build_arch {
        let issue = LintianIssue::source_with_info(
            "debian-rules-missing-recommended-target",
            vec!["build-arch [debian/rules]".to_string()],
        );
        if !issue.should_fix(base_path) {
            return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
        }

        added.push("build-arch");
        fixed_issues.push(issue);
        let mut rule = makefile.add_rule("build-arch");

        // If there are non-all architectures, make it depend on "build"
        if archs.iter().any(|a| a != "all") {
            rule.add_prerequisite("build")
                .map_err(|e| FixerError::Other(format!("Failed to add prerequisite: {}", e)))?;
        }
    }

    if added.is_empty() {
        return Err(FixerError::NoChanges);
    }

    // Add to .PHONY if it exists
    if let Some(_phony_rule) = makefile.find_rule_by_target(".PHONY") {
        for target in &added {
            let _ = makefile.add_phony_target(target);
        }
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    let description = if added.len() == 1 {
        format!("Add missing debian/rules target {}.", added[0])
    } else {
        format!("Add missing debian/rules targets {}.", added.join(", "))
    };

    Ok(FixerResult::builder(description)
        .fixed_issues(fixed_issues)
        .build())
}

declare_fixer! {
    name: "debian-rules-missing-recommended-target",
    tags: ["debian-rules-missing-recommended-target"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}
