use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::licenses::COMMON_LICENSES_DIR;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use regex::Regex;
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    detect_with_licenses_dir(ws, Path::new(COMMON_LICENSES_DIR))
}

/// Whether `name` is a license that ships uncompressed in `licenses_dir`
/// (the real /usr/share/common-licenses in production). Checking the
/// filesystem matches how the other copyright fixers recognise common
/// licenses and avoids hardcoding lintian's list, which drifts over time.
fn is_known_common_license(licenses_dir: &Path, name: &str) -> bool {
    licenses_dir.join(name).is_file()
}

fn detect_with_licenses_dir(
    ws: &dyn Workspace,
    licenses_dir: &Path,
) -> Result<Vec<Diagnostic>, FixerError> {
    let copyright_rel = PathBuf::from("debian/copyright");
    let Some(bytes) = ws.read_file(&copyright_rel)? else {
        return Ok(Vec::new());
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };

    let pattern = Regex::new(r"usr/share/common-licenses/([A-Za-z0-9.+-]+)\.gz").unwrap();

    // One diagnostic per distinct compressed reference. Each carries its own
    // Substitute, which replaces every occurrence of that path -- so a
    // reference repeated in the file is still fixed in full.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for caps in pattern.captures_iter(content) {
        let path = caps[0].to_string();
        let license = &caps[1];
        if !is_known_common_license(licenses_dir, license) || seen.contains(&path) {
            continue;
        }
        seen.push(path.clone());

        let uncompressed = path.trim_end_matches(".gz").to_string();
        let issue = LintianIssue::source_with_info(
            "copyright-refers-to-compressed-license",
            Visibility::Error,
            vec![path.clone()],
        );
        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                "debian/copyright refers to compressed common-licenses file.",
                "Refer to uncompressed license file in /usr/share/common-licenses.",
                vec![Action::Filesystem(FilesystemAction::Substitute {
                    file: copyright_rel.clone(),
                    from: path,
                    to: uncompressed,
                })],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "copyright-refers-to-compressed-license",
    tags: ["copyright-refers-to-compressed-license"],
    triggers: [debian_workspace::Trigger::File("debian/copyright")],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_workspace::fs_workspace::FsWorkspace;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// A licenses dir populated with the names a real system ships, so the
    /// recognition check doesn't depend on the host's /usr/share.
    fn fake_licenses_dir(tmp: &TempDir) -> PathBuf {
        let dir = tmp.path().join("common-licenses");
        fs::create_dir_all(&dir).unwrap();
        for name in ["GPL", "GPL-2", "GPL-3", "LGPL-2.1", "Apache-2.0"] {
            fs::write(dir.join(name), b"").unwrap();
        }
        dir
    }

    fn detect_in(base: &Path, licenses_dir: &Path) -> Result<Vec<Diagnostic>, FixerError> {
        let ws = FsWorkspace::new(base, Some("test".into()), Some("1.0".parse().unwrap()));
        detect_with_licenses_dir(&ws, licenses_dir)
    }

    fn write_copyright(base: &Path, content: &str) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("copyright"), content).unwrap();
    }

    #[test]
    fn test_detects_compressed_reference() {
        let tmp = TempDir::new().unwrap();
        let licenses = fake_licenses_dir(&tmp);
        let base = tmp.path().join("pkg");
        write_copyright(
            &base,
            "License: GPL-2\n See /usr/share/common-licenses/GPL-2.gz for details.\n",
        );

        let diags = detect_in(&base, &licenses).unwrap();
        assert_eq!(diags.len(), 1);
        let issue = diags[0].issue.as_ref().unwrap();
        assert_eq!(
            issue.tag.as_deref(),
            Some("copyright-refers-to-compressed-license")
        );
        assert_eq!(
            issue.info.as_deref(),
            Some("usr/share/common-licenses/GPL-2.gz")
        );
        assert_eq!(diags[0].certainty, Some(Certainty::Certain));
        assert_eq!(
            diags[0].plans[0].actions,
            vec![Action::Filesystem(FilesystemAction::Substitute {
                file: PathBuf::from("debian/copyright"),
                from: "usr/share/common-licenses/GPL-2.gz".into(),
                to: "usr/share/common-licenses/GPL-2".into(),
            })]
        );
    }

    #[test]
    fn test_detects_multiple_distinct_references() {
        let tmp = TempDir::new().unwrap();
        let licenses = fake_licenses_dir(&tmp);
        let base = tmp.path().join("pkg");
        write_copyright(
            &base,
            "See /usr/share/common-licenses/GPL.gz and \
             /usr/share/common-licenses/LGPL-2.1.gz.\n",
        );

        let diags = detect_in(&base, &licenses).unwrap();
        assert_eq!(diags.len(), 2);
        // Each diagnostic fixes its own reference.
        assert_eq!(diags[0].plans[0].actions.len(), 1);
        assert_eq!(diags[1].plans[0].actions.len(), 1);
    }

    #[test]
    fn test_no_change_for_uncompressed() {
        let tmp = TempDir::new().unwrap();
        let licenses = fake_licenses_dir(&tmp);
        let base = tmp.path().join("pkg");
        write_copyright(&base, "See /usr/share/common-licenses/GPL-3 for details.\n");

        assert!(detect_in(&base, &licenses).unwrap().is_empty());
    }

    #[test]
    fn test_ignores_unknown_license() {
        // A `.gz` whose uncompressed counterpart is not in the common-licenses
        // dir is left alone, rather than relying on a hardcoded name list.
        let tmp = TempDir::new().unwrap();
        let licenses = fake_licenses_dir(&tmp);
        let base = tmp.path().join("pkg");
        write_copyright(
            &base,
            "See /usr/share/common-licenses/MPL-2.0.gz for details.\n",
        );

        assert!(detect_in(&base, &licenses).unwrap().is_empty());
    }

    #[test]
    fn test_no_copyright_file() {
        let tmp = TempDir::new().unwrap();
        let licenses = fake_licenses_dir(&tmp);
        let base = tmp.path().join("pkg");
        fs::create_dir_all(&base).unwrap();
        assert!(detect_in(&base, &licenses).unwrap().is_empty());
    }
}
