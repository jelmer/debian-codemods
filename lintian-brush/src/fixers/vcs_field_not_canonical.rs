use crate::diagnostic::{Action, Deb822Action, DebcargoAction, Diagnostic, ParagraphSelector};
use crate::{FixerError, LintianIssue};
use debian_control::lossless::Control;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const VCS_TYPES: &[&str] = &[
    "Git", "Browser", "Svn", "Bzr", "Hg", "Cvs", "Arch", "Darcs", "Mtn", "Svk",
];

fn canonicalize_vcs_url(vcs_type: &str, url: &str) -> String {
    match vcs_type {
        "Browser" => debian_analyzer::vcs::canonicalize_vcs_browser_url(url),
        "Git" => match url.parse::<debian_control::vcs::ParsedVcs>() {
            Ok(mut parsed) => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                if let Ok(repo_url) = url::Url::parse(&parsed.repo_url) {
                    if let Some(canonical_url) = rt.block_on(
                        upstream_ontologist::vcs::canonical_git_repo_url(&repo_url, None),
                    ) {
                        parsed.repo_url = canonical_url.to_string();
                    }
                }
                parsed.to_string()
            }
            Err(_) => url.to_string(),
        },
        _ => url.to_string(),
    }
}

/// Per-diagnostic message tag, threaded through to the describer.
const SEP: char = '\t';

pub fn detect(base_path: &Path) -> Result<Vec<Diagnostic>, FixerError> {
    let debcargo_rel = PathBuf::from("debian/debcargo.toml");
    let control_rel = PathBuf::from("debian/control");
    let debcargo_abs = base_path.join(&debcargo_rel);
    let control_abs = base_path.join(&control_rel);

    if debcargo_abs.exists() && !control_abs.exists() {
        // Debcargo branch — fields live in [source] under TOML keys
        // vcs_git / vcs_browser. We canonicalize whichever is set.
        let toml_text = std::fs::read_to_string(&debcargo_abs)?;
        let doc: toml_edit::DocumentMut = toml_text
            .parse()
            .map_err(|e| FixerError::Other(format!("Failed to parse debcargo.toml: {}", e)))?;
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let candidates: &[(&str, &str)] = &[("vcs_git", "Git"), ("vcs_browser", "Browser")];
        if let Some(source) = doc.get("source").and_then(|s| s.as_table()) {
            for (toml_key, vcs_type) in candidates {
                let Some(url) = source.get(toml_key).and_then(|v| v.as_str()) else {
                    continue;
                };
                let new_value = canonicalize_vcs_url(vcs_type, url);
                if new_value == url {
                    continue;
                }
                let issue = LintianIssue::source_with_info(
                    "vcs-field-not-canonical",
                    vec![format!("{} {} {}", vcs_type, url, new_value)],
                );
                let field_name = format!("Vcs-{}", vcs_type);
                diagnostics.push(Diagnostic::with_actions(
                    issue,
                    format!("{}{}{}", "set", SEP, field_name),
                    vec![Action::Debcargo(DebcargoAction::SetSourceField {
                        file: debcargo_rel.clone(),
                        field: (*toml_key).to_string(),
                        value: new_value,
                    })],
                ));
            }
        }
        return Ok(diagnostics);
    }

    if !control_abs.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&control_abs)?;
    let Ok(control) = Control::from_str(&content) else {
        return Ok(Vec::new());
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let p = source.as_deb822();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for vcs_type in VCS_TYPES {
        let field_name = format!("Vcs-{}", vcs_type);
        let Some(url) = p.get(&field_name) else {
            continue;
        };
        let new_value = canonicalize_vcs_url(vcs_type, &url);
        if new_value == url {
            continue;
        }
        let issue = LintianIssue::source_with_info(
            "vcs-field-not-canonical",
            vec![format!("{} {} {}", vcs_type, url, new_value)],
        );
        diagnostics.push(Diagnostic::with_actions(
            issue,
            format!("{}{}{}", "set", SEP, field_name),
            vec![Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: field_name,
                value: new_value,
            })],
        ));
    }
    Ok(diagnostics)
}

fn describe_aggregate(fixed: &[Diagnostic], _actions: &[Action]) -> String {
    let mut fields = BTreeSet::new();
    for d in fixed {
        if let Some((tag, field)) = d.message.split_once(SEP) {
            if tag == "set" {
                fields.insert(field.to_string());
            }
        }
    }
    let list = fields.into_iter().collect::<Vec<_>>().join(", ");
    format!("Use canonical URL in {}.", list)
}

declare_fixer! {
    name: "vcs-field-not-canonical",
    tags: ["vcs-field-not-canonical"],
    after: ["vcs-field-mismatch"],
    before: ["vcs-field-uses-insecure-uri"],
    diagnose: |basedir, _package, _version, _preferences| {
        detect(basedir)
    },
    describe: |fixed, actions| {
        describe_aggregate(fixed, actions)
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
        let v: Version = "1.0".parse().unwrap();
        FixerImpl.apply(base, "test", &v, &FixerPreferences::default())
    }

    #[test]
    fn test_canonicalize_browser_url() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nVcs-Browser: https://bzr.debian.org/loggerhead/pkg-bazaar/bzr\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Use canonical URL in Vcs-Browser.");
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nVcs-Browser: https://anonscm.debian.org/loggerhead/pkg-bazaar/bzr\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_no_change_git_url() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: git://github.com/user/repo.git\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_canonical() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: test-package\nVcs-Git: https://github.com/user/repo.git\nVcs-Browser: https://github.com/user/repo\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_multiple_vcs_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nVcs-Git: git://salsa.debian.org/team/package\nVcs-Browser: https://bzr.debian.org/loggerhead/pkg-bazaar/bzr\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(
            result.description,
            "Use canonical URL in Vcs-Browser, Vcs-Git."
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nVcs-Git: git://salsa.debian.org/team/package.git\nVcs-Browser: https://anonscm.debian.org/loggerhead/pkg-bazaar/bzr\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        );
    }

    #[test]
    fn test_salsa_git_url() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        let control_path = debian.join("control");
        fs::write(
            &control_path,
            "Source: test-package\nVcs-Git: https://salsa.debian.org/team/package\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        )
        .unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.description, "Use canonical URL in Vcs-Git.");
        assert_eq!(
            fs::read_to_string(&control_path).unwrap(),
            "Source: test-package\nVcs-Git: https://salsa.debian.org/team/package.git\n\nPackage: test-package\nDescription: Test package\n This is a test package.\n",
        );
    }
}
