use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use std::path::PathBuf;

const PKG_PERL_EMAIL: &str = "pkg-perl-maintainers@lists.alioth.debian.org";
const URL_BASE: &str = "https://salsa.debian.org/perl-team/modules/packages";

fn extract_email(addr: &str) -> &str {
    if let (Some(start), Some(end)) = (addr.rfind('<'), addr.rfind('>')) {
        if end > start {
            return &addr[start + 1..end];
        }
    }
    addr
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();

    let Some(maintainer) = paragraph.get("Maintainer") else {
        return Ok(Vec::new());
    };
    if extract_email(&maintainer) != PKG_PERL_EMAIL {
        return Ok(Vec::new());
    }
    let Some(source_name) = paragraph.get("Source") else {
        return Ok(Vec::new());
    };

    let old_vcs_git = paragraph.get("Vcs-Git");
    let old_vcs_browser = paragraph.get("Vcs-Browser");
    let target_git = format!("{}/{}.git", URL_BASE, source_name);
    let target_browser = format!("{}/{}", URL_BASE, source_name);

    let mut diagnostics = Vec::new();

    // Diagnostic 1: Vcs-Git/Vcs-Browser don't use the canonical team URL.
    let needs_team_url = old_vcs_git
        .as_ref()
        .is_none_or(|v| !v.starts_with(URL_BASE))
        || old_vcs_browser
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE));
    if needs_team_url {
        // Emit in SOURCE_FIELD_ORDER (Vcs-Browser before Vcs-Git) so the
        // canonical-ordering insert in the applier lands them in the
        // right relative position.
        let mut actions = Vec::new();
        if old_vcs_browser
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE))
        {
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: "Vcs-Browser".into(),
                value: target_browser,
            }));
        }
        if old_vcs_git
            .as_ref()
            .is_none_or(|v| !v.starts_with(URL_BASE))
        {
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: "Vcs-Git".into(),
                value: target_git,
            }));
        }
        diagnostics.push(
            Diagnostic::with_actions(
                LintianIssue::source("team/pkg-perl/vcs/no-team-url"),
                "Use standard Vcs fields for perl package.",
                actions,
            )
            .with_certainty(Certainty::Certain),
        );
    }

    // Diagnostic 2 (one per field): non-Git/non-Browser Vcs-* fields. The
    // pkg-perl team workflow only uses Vcs-Git and Vcs-Browser, so
    // anything else is stale.
    for key in paragraph.keys() {
        let lower = key.to_lowercase();
        if !lower.starts_with("vcs-") || lower == "vcs-git" || lower == "vcs-browser" {
            continue;
        }
        let value = paragraph.get(&key).unwrap_or_default();
        diagnostics.push(
            Diagnostic::with_actions(
                LintianIssue::source_with_info(
                    "team/pkg-perl/vcs/no-git",
                    vec![format!("{} {}", key, value)],
                ),
                "Use standard Vcs fields for perl package.",
                vec![Action::Deb822(Deb822Action::RemoveField {
                    file: control_rel.clone(),
                    paragraph: ParagraphSelector::Source,
                    field: key,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "pkg-perl-vcs",
    tags: ["team/pkg-perl/vcs/no-team-url", "team/pkg-perl/vcs/no-git"],
    triggers: [
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Source",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-*",
        },
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "libfoo-perl", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_sets_vcs_fields_for_pkg_perl() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(
            &path,
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl\nVcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git\n\nPackage: libfoo-perl\nDescription: test\n",
        );
    }

    #[test]
    fn test_no_change_when_already_correct() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        let original = "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl\nVcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git\n\nPackage: libfoo-perl\nDescription: test\n";
        fs::write(&path, original).unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn test_no_change_when_not_pkg_perl() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(
            &path,
            "Source: libfoo-perl\nMaintainer: Someone Else <someone@example.com>\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_removes_non_git_vcs_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(
            &path,
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Svn: https://old-url.example.com\n\nPackage: libfoo-perl\nDescription: test\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Source: libfoo-perl\nMaintainer: Debian Perl Group <pkg-perl-maintainers@lists.alioth.debian.org>\nVcs-Browser: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl\nVcs-Git: https://salsa.debian.org/perl-team/modules/packages/libfoo-perl.git\n\nPackage: libfoo-perl\nDescription: test\n",
        );
    }
}
