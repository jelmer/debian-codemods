use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Pattern for the commented-out `Vcs-*` lines that old `dh_make`
/// versions appended to `debian/control`. They point at the long-defunct
/// `git.debian.org` `collab-maint` repositories and still carry the
/// unsubstituted `<pkg>` placeholder. Ported from lintian's
/// `template/dh-make/control/vcs` check.
fn dh_make_vcs_regex() -> Regex {
    Regex::new(
        r"^#\s*Vcs-(?:Git|Browser):\s*(?:git|http)://git\.debian\.org/(?:\?p=)?collab-maint/<pkg>\.git",
    )
    .unwrap()
}

/// Scan one control paragraph for fields whose value carries embedded,
/// commented-out dh_make `Vcs-*` lines.
///
/// deb822 keeps `#`-prefixed lines that follow a field's value attached
/// to that field; `dh_make` emits its `Vcs-*` comments right after
/// `Homepage`, so they end up embedded there. For each field that only
/// carries dh_make boilerplate comments, push a [`Deb822Action::DropFieldComments`]
/// action and record the first matching comment line.
fn scan_paragraph(
    paragraph: &deb822_lossless::Paragraph,
    selector: &ParagraphSelector,
    re: &Regex,
    control_rel: &Path,
    actions: &mut Vec<Action>,
    first_match: &mut Option<String>,
) {
    for entry in paragraph.entries() {
        let Some(field) = entry.key() else {
            continue;
        };
        // `value_with_comments` renders the field value together with any
        // `#`-prefixed lines the parser kept attached to it.
        let with_comments = entry.value_with_comments();
        let comment_lines: Vec<&str> = with_comments
            .lines()
            .filter(|line| line.starts_with('#'))
            .collect();
        let matching: Vec<&str> = comment_lines
            .iter()
            .copied()
            .filter(|line| re.is_match(line))
            .collect();
        if matching.is_empty() {
            continue;
        }
        // Only act when *every* embedded comment is dh_make Vcs boilerplate:
        // rewriting the field drops all of its embedded comments, so a
        // foreign comment mixed in would be lost too.
        if matching.len() != comment_lines.len() {
            continue;
        }
        if first_match.is_none() {
            *first_match = Some(matching[0].to_string());
        }
        actions.push(Action::Deb822(Deb822Action::DropFieldComments {
            file: control_rel.to_path_buf(),
            paragraph: selector.clone(),
            field,
        }));
    }
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let control_rel = PathBuf::from("debian/control");
    let re = dh_make_vcs_regex();
    let mut actions = Vec::new();
    let mut first_match = None;

    if let Some(source) = control.source() {
        scan_paragraph(
            source.as_deb822(),
            &ParagraphSelector::Source,
            &re,
            &control_rel,
            &mut actions,
            &mut first_match,
        );
    }
    for binary in control.binaries() {
        let Some(package) = binary.as_deb822().get("Package") else {
            continue;
        };
        scan_paragraph(
            binary.as_deb822(),
            &ParagraphSelector::Binary { package },
            &re,
            &control_rel,
            &mut actions,
            &mut first_match,
        );
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let issue = LintianIssue::source_with_info(
        "control-file-contains-dh-make-vcs-comment",
        Visibility::Warning,
        vec![first_match.expect("a non-empty action set implies a matched comment")],
    );
    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/control contains commented-out dh_make Vcs lines.",
        "Remove commented-out dh_make Vcs lines from debian/control.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "control-file-contains-dh_make-vcs-comment",
    tags: ["control-file-contains-dh-make-vcs-comment"],
    triggers: [
        debian_workspace::Trigger::File("debian/control"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_control(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), content).unwrap();
    }

    #[test]
    fn test_removes_dh_make_vcs_comments() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\n\
             Build-Depends: debhelper-compat (= 13)\n\
             Homepage: https://example.com/\n\
             #Vcs-Git: git://git.debian.org/collab-maint/<pkg>.git\n\
             #Vcs-Browser: http://git.debian.org/?p=collab-maint/<pkg>.git\n\
             \n\
             Package: test\n\
             Architecture: any\n\
             Depends: ${shlibs:Depends}, ${misc:Depends}\n",
        );

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag.as_deref(),
            Some("control-file-contains-dh-make-vcs-comment")
        );

        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/control")).unwrap(),
            "Source: test\n\
             Build-Depends: debhelper-compat (= 13)\n\
             Homepage: https://example.com/\n\
             \n\
             Package: test\n\
             Architecture: any\n\
             Depends: ${shlibs:Depends}, ${misc:Depends}\n",
        );
    }

    #[test]
    fn test_no_change_without_comments() {
        let tmp = TempDir::new().unwrap();
        write_control(
            tmp.path(),
            "Source: test\nHomepage: https://example.com/\n\nPackage: test\nArchitecture: all\n",
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_unrelated_commented_relations() {
        // A commented-out build dependency is not dh_make boilerplate and
        // must be left untouched.
        let tmp = TempDir::new().unwrap();
        let original = "Source: test\n\
             Build-Depends:\n\
             \u{20}debhelper-compat (= 13),\n\
             #python3-nose,\n\
             \u{20}python3\n\
             \n\
             Package: test\n\
             Architecture: all\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_keeps_foreign_comment_in_same_field() {
        // When a field carries a foreign comment alongside the dh_make
        // boilerplate, rewriting it would drop the foreign comment too, so
        // the fixer conservatively leaves the field alone.
        let tmp = TempDir::new().unwrap();
        let original = "Source: test\n\
             Homepage: https://example.com/\n\
             # upstream moved hosts\n\
             #Vcs-Git: git://git.debian.org/collab-maint/<pkg>.git\n\
             \n\
             Package: test\n\
             Architecture: all\n";
        write_control(tmp.path(), original);
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
