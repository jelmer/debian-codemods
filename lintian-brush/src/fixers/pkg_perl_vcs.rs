use crate::{FixerError, FixerResult, LintianIssue};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use std::path::Path;

const PKG_PERL_EMAIL: &str = "pkg-perl-maintainers@lists.alioth.debian.org";
const URL_BASE: &str = "https://salsa.debian.org/perl-team/modules/packages";

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let editor = TemplatedControlEditor::open(&control_path)?;

    // Parse the maintainer field and extract the email address
    let maintainer = if let Some(source) = editor.source() {
        source
            .as_deb822()
            .get("Maintainer")
            .ok_or(FixerError::NoChanges)?
    } else {
        return Err(FixerError::NoChanges);
    };

    // Extract email from "Name <email>" format
    let email = if let Some(start) = maintainer.rfind('<') {
        if let Some(end) = maintainer.rfind('>') {
            &maintainer[start + 1..end]
        } else {
            &maintainer
        }
    } else {
        &maintainer
    };

    if email != PKG_PERL_EMAIL {
        // Nothing to do here, it's not a pkg-perl-maintained package
        return Err(FixerError::NoChanges);
    }

    // Get source package name
    let source_name = if let Some(source) = editor.source() {
        source.name().ok_or(FixerError::NoChanges)?
    } else {
        return Err(FixerError::NoChanges);
    };

    let Some(mut source) = editor.source() else {
        return Err(FixerError::NoChanges);
    };

    // Get old values before any manipulation
    let old_vcs_git = source.get_vcs_url("Git");
    let old_vcs_browser = source.get_vcs_url("Browser");

    // Set standard Vcs fields
    let vcs_git_url = format!("{}/{}.git", URL_BASE, source_name);
    let vcs_browser_url = format!("{}/{}", URL_BASE, source_name);

    let mut made_changes = false;
    let mut removed_non_git_vcs = false;
    let mut fixed_urls = false;
    let mut overridden_issues = Vec::new();

    // Determine what changes need to be made
    let need_to_fix_urls = old_vcs_git
        .as_ref()
        .is_none_or(|v| !v.starts_with(URL_BASE))
        || old_vcs_browser
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE));

    let fields_to_remove: Vec<String> = {
        let paragraph = source.as_deb822();
        paragraph
            .keys()
            .filter(|field| {
                let lower = field.to_lowercase();
                lower.starts_with("vcs-") && lower != "vcs-git" && lower != "vcs-browser"
            })
            .collect()
    };

    let need_to_remove_non_git_vcs = !fields_to_remove.is_empty();

    // Check for overrides
    if need_to_fix_urls {
        let issue = LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            tag: Some("team/pkg-perl/vcs/no-team-url".to_string()),
            info: None,
        };
        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
        } else {
            fixed_urls = true;
        }
    }

    if need_to_remove_non_git_vcs {
        let issue = LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            tag: Some("team/pkg-perl/vcs/no-git".to_string()),
            info: None,
        };
        if !issue.should_fix(base_path) {
            overridden_issues.push(issue);
        } else {
            removed_non_git_vcs = true;
        }
    }

    // If all issues are overridden, return NoChangesAfterOverrides
    if !fixed_urls && !removed_non_git_vcs {
        if overridden_issues.is_empty() {
            return Err(FixerError::NoChanges);
        } else {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
    }

    // Remove all VCS fields except Git and Browser (if not overridden)
    if removed_non_git_vcs {
        let paragraph = source.as_mut_deb822();
        for field in &fields_to_remove {
            paragraph.remove(field);
        }
        made_changes = true;
    }

    // Only update if the value doesn't already start with URL_BASE (and not overridden)
    if fixed_urls {
        if old_vcs_git
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE))
        {
            source.set_vcs_url("Git", &vcs_git_url);
            made_changes = true;
        }

        if old_vcs_browser
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE))
        {
            source.set_vcs_url("Browser", &vcs_browser_url);
            made_changes = true;
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    editor.commit()?;

    let mut fixed_tags = Vec::new();
    if fixed_urls {
        fixed_tags.push("team/pkg-perl/vcs/no-team-url");
    }
    if removed_non_git_vcs {
        fixed_tags.push("team/pkg-perl/vcs/no-git");
    }

    Ok(
        FixerResult::builder("Use standard Vcs fields for perl package.")
            .certainty(crate::Certainty::Certain)
            .fixed_tags(fixed_tags)
            .build(),
    )
}

crate::declare_fixer! {
    name: "pkg-perl-vcs",
    tags: ["team/pkg-perl/vcs/no-team-url", "team/pkg-perl/vcs/no-git"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_sets_vcs_fields_for_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(updated_content.contains(
            "Vcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git"
        ));
        assert!(updated_content.contains(
            "Vcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl"
        ));
    }

    #[test]
    fn test_no_change_when_already_correct() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl\nVcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_not_pkg_perl() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Someone Else <someone@example.com>\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_non_git_vcs_fields() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let control_content = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Svn: https://old-url.example.com\n\nPackage: libfoo-perl\nDescription: test\n";
        let control_path = debian_dir.join("control");
        fs::write(&control_path, control_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "libfoo-perl",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&control_path).unwrap();
        assert!(!updated_content.contains("Vcs-Svn"));
        assert!(updated_content.contains(
            "Vcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git"
        ));
    }
}
