use crate::debhelper::{
    extract_subst_vars, has_dh_exec_filter, has_dh_exec_rename, is_native_subst_var,
    read_dh_exec_script,
};
use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_workspace::Workspace;
use std::path::Path;

/// Debhelper config-file categories whose contents are lists of filenames
/// (from lintian's `debhelper/filename-config-files` data file). Only these
/// are checked for dh-exec usage.
const FILENAME_CONFIGS: &[&str] = &[
    "dirs",
    "docs",
    "examples",
    "info",
    "install",
    "links",
    "manpages",
    "mime",
    "sgmlcatalogs",
    "sharedmimeinfo",
    "wm",
];

/// A dh-exec feature detected in the body of a config file.
#[derive(Debug, PartialEq, Eq)]
enum DhExecFeature {
    /// A `src => dest` rename form.
    Install,
    /// An `[arch]` architecture filter or `<profile>` build-profile filter.
    Filter,
    /// A `${VAR}` variable substitution that dh_install/dh_installmanpages
    /// cannot itself perform at the current debhelper compat level.
    Subst,
}

/// Scan a debhelper config file body for dh-exec features.
///
/// `debhelper_level` is the package's debhelper compat level, or `None` when
/// unknown. From compat 13 onwards `dh_install` and friends substitute the
/// common `DEB_*` variables themselves, so `${DEB_HOST_MULTIARCH}` alone no
/// longer requires dh-exec.
fn features_used(content: &str, debhelper_level: Option<u8>) -> Vec<DhExecFeature> {
    let mut found = Vec::new();
    let native_subst = debhelper_level.map(|l| l >= 13).unwrap_or(false);
    for raw_line in content.lines() {
        // Skip the shebang and comment/blank lines.
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if has_dh_exec_rename(line) && !found.contains(&DhExecFeature::Install) {
            found.push(DhExecFeature::Install);
        }
        if has_dh_exec_filter(line) && !found.contains(&DhExecFeature::Filter) {
            found.push(DhExecFeature::Filter);
        }
        for var in extract_subst_vars(line) {
            // A ${VAR} the applier cannot handle natively is a reason to keep
            // dh-exec. If it *is* a well-known DEB_* var, only count it as a
            // feature when the compat level doesn't let debhelper substitute
            // it itself.
            if !is_native_subst_var(&var) || !native_subst {
                if !found.contains(&DhExecFeature::Subst) {
                    found.push(DhExecFeature::Subst);
                }
                break;
            }
        }
    }
    found
}

/// Basenames the fixer should consider. Returns the category (the part after
/// the last `.`, or the whole basename when there is no dot) if the file
/// matches one of `FILENAME_CONFIGS`.
fn category_for_basename(basename: &str) -> Option<&'static str> {
    let candidate = match basename.rsplit_once('.') {
        Some((_, ext)) => ext,
        None => basename,
    };
    FILENAME_CONFIGS.iter().copied().find(|c| *c == candidate)
}

pub fn detect(
    ws: &dyn Workspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let debian = Path::new("debian");
    let entries = match ws.list_dir(debian)? {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let debhelper_level = debian_workspace::compat_level(ws)?;

    let mut diagnostics = Vec::new();
    for name in entries {
        if category_for_basename(&name).is_none() {
            continue;
        }
        let rel = debian.join(&name);
        let Some((mode, body)) = read_dh_exec_script(ws, &rel)? else {
            continue;
        };

        let features = features_used(&body, debhelper_level);
        if !features.is_empty() {
            // Something is actually using dh-exec; leave it alone.
            continue;
        }

        // The fix: drop the shebang line, then clear the executable bits.
        let info = format!("[{}]", rel.display());
        let issue = LintianIssue::source_with_info(
            "dh-exec-script-without-dh-exec-features",
            Visibility::Warning,
            vec![info],
        );

        let description = format!("Remove unused dh-exec usage from {}.", rel.display());

        diagnostics.push(
            Diagnostic::with_actions(
                issue,
                description.clone(),
                description,
                vec![
                    Action::Filesystem(FilesystemAction::Write {
                        file: rel.clone(),
                        content: body.into_bytes(),
                    }),
                    Action::Filesystem(FilesystemAction::SetMode {
                        file: rel,
                        mode: mode & !0o111,
                    }),
                ],
            )
            .with_certainty(Certainty::Certain),
        );
    }

    Ok(diagnostics)
}

declare_detector! {
    name: "dh-exec-script-without-dh-exec-features",
    tags: ["dh-exec-script-without-dh-exec-features"],
    triggers: [
        debian_workspace::Trigger::Glob("debian/*.dirs"),
        debian_workspace::Trigger::Glob("debian/*.docs"),
        debian_workspace::Trigger::Glob("debian/*.examples"),
        debian_workspace::Trigger::Glob("debian/*.info"),
        debian_workspace::Trigger::Glob("debian/*.install"),
        debian_workspace::Trigger::Glob("debian/*.links"),
        debian_workspace::Trigger::Glob("debian/*.manpages"),
        debian_workspace::Trigger::Glob("debian/*.mime"),
        debian_workspace::Trigger::Glob("debian/*.sgmlcatalogs"),
        debian_workspace::Trigger::Glob("debian/*.sharedmimeinfo"),
        debian_workspace::Trigger::Glob("debian/*.wm"),
        debian_workspace::Trigger::File("debian/dirs"),
        debian_workspace::Trigger::File("debian/docs"),
        debian_workspace::Trigger::File("debian/examples"),
        debian_workspace::Trigger::File("debian/info"),
        debian_workspace::Trigger::File("debian/install"),
        debian_workspace::Trigger::File("debian/links"),
        debian_workspace::Trigger::File("debian/manpages"),
        debian_workspace::Trigger::File("debian/mime"),
        debian_workspace::Trigger::File("debian/sgmlcatalogs"),
        debian_workspace::Trigger::File("debian/sharedmimeinfo"),
        debian_workspace::Trigger::File("debian/wm"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorImpl;
        let ws = debian_workspace::fs_workspace::FsWorkspace::new(
            base,
            Some("test".into()),
            Some(version.clone()),
        );
        adapter.apply(&ws, &FixerPreferences::default())
    }

    fn write_pkg(base: &Path, install_body: &str, mode: u32, compat: Option<u8>) {
        let debian = base.join("debian");
        fs::create_dir_all(&debian).unwrap();
        let install = debian.join("foo.install");
        fs::write(&install, install_body).unwrap();
        let mut perms = fs::metadata(&install).unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(&install, perms).unwrap();
        if let Some(level) = compat {
            fs::write(debian.join("compat"), format!("{}\n", level)).unwrap();
        }
    }

    #[test]
    fn test_features_used_plain() {
        assert!(features_used("usr/bin/foo\nusr/share/foo/*\n", Some(13)).is_empty());
    }

    #[test]
    fn test_features_used_install() {
        assert_eq!(
            features_used("upstream-name => usr/bin/foo\n", Some(13)),
            vec![DhExecFeature::Install]
        );
    }

    #[test]
    fn test_features_used_arch_filter() {
        assert_eq!(
            features_used("usr/bin/foo [amd64]\n", Some(13)),
            vec![DhExecFeature::Filter]
        );
    }

    #[test]
    fn test_features_used_profile_filter() {
        assert_eq!(
            features_used("usr/bin/foo <!nodoc>\n", Some(13)),
            vec![DhExecFeature::Filter]
        );
    }

    #[test]
    fn test_features_used_native_subst_on_compat13() {
        // At compat 13, DEB_HOST_MULTIARCH is not a dh-exec feature.
        assert!(features_used("usr/lib/${DEB_HOST_MULTIARCH}/*\n", Some(13)).is_empty());
    }

    #[test]
    fn test_features_used_native_subst_on_compat12() {
        // At compat 12, DEB_HOST_MULTIARCH still requires dh-exec.
        assert_eq!(
            features_used("usr/lib/${DEB_HOST_MULTIARCH}/*\n", Some(12)),
            vec![DhExecFeature::Subst]
        );
    }

    #[test]
    fn test_features_used_unknown_subst() {
        // Unknown vars always keep dh-exec in play.
        assert_eq!(
            features_used("usr/bin/${MY_TOOL}\n", Some(13)),
            vec![DhExecFeature::Subst]
        );
    }

    #[test]
    fn test_features_used_ignores_comments() {
        assert!(features_used("# foo => bar\n# [amd64]\nusr/bin/foo\n", Some(13)).is_empty());
    }

    #[test]
    fn test_category_for_basename() {
        assert_eq!(category_for_basename("install"), Some("install"));
        assert_eq!(category_for_basename("foo.install"), Some("install"));
        assert_eq!(category_for_basename("foo.manpages"), Some("manpages"));
        assert_eq!(category_for_basename("control"), None);
        assert_eq!(category_for_basename("rules"), None);
    }

    #[test]
    fn test_fixes_plain_install_at_compat13() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "#!/usr/bin/dh-exec\nusr/bin/foo\nusr/share/foo/*\n",
            0o755,
            Some(13),
        );
        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            result.fixed_lintian_issues[0].tag,
            Some("dh-exec-script-without-dh-exec-features".to_string())
        );
        assert_eq!(
            result.fixed_lintian_issues[0].info,
            Some("[debian/foo.install]".to_string())
        );
        let install = tmp.path().join("debian/foo.install");
        assert_eq!(
            fs::read_to_string(&install).unwrap(),
            "usr/bin/foo\nusr/share/foo/*\n"
        );
        assert_eq!(
            fs::metadata(&install).unwrap().permissions().mode() & 0o111,
            0,
        );
    }

    #[test]
    fn test_fixes_plain_install_at_compat13_via_bd() {
        // With no debian/compat, compat_level falls back to the
        // debhelper-compat build-dep.
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\nArchitecture: any\nDescription: x\n y\n",
        )
        .unwrap();
        let install = debian.join("install");
        fs::write(
            &install,
            "#!/usr/bin/dh-exec\nusr/lib/${DEB_HOST_MULTIARCH}/*\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&install).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&install, perms).unwrap();

        let result = run_apply(tmp.path()).unwrap();
        assert_eq!(result.fixed_lintian_issues.len(), 1);
        assert_eq!(
            fs::read_to_string(&install).unwrap(),
            "usr/lib/${DEB_HOST_MULTIARCH}/*\n"
        );
    }

    #[test]
    fn test_backs_off_when_install_uses_arch_filter() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "#!/usr/bin/dh-exec\nusr/bin/foo [amd64]\n",
            0o755,
            Some(13),
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_backs_off_when_install_uses_rename() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "#!/usr/bin/dh-exec\nfoo => usr/bin/bar\n",
            0o755,
            Some(13),
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_backs_off_on_compat12_with_deb_host_multiarch() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "#!/usr/bin/dh-exec\nusr/lib/${DEB_HOST_MULTIARCH}/*\n",
            0o755,
            Some(12),
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_non_executable() {
        let tmp = TempDir::new().unwrap();
        write_pkg(
            tmp.path(),
            "#!/usr/bin/dh-exec\nusr/bin/foo\n",
            0o644,
            Some(13),
        );
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_file_without_shebang() {
        let tmp = TempDir::new().unwrap();
        write_pkg(tmp.path(), "usr/bin/foo\n", 0o755, Some(13));
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_shebang_without_dh_exec() {
        let tmp = TempDir::new().unwrap();
        write_pkg(tmp.path(), "#!/bin/sh\necho usr/bin/foo\n", 0o755, Some(13));
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_ignores_directory_with_config_name() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian/examples")).unwrap();
        fs::write(tmp.path().join("debian/compat"), "13\n").unwrap();
        let result = run_apply(tmp.path());
        assert!(matches!(result, Err(FixerError::NoChanges)), "{:?}", result);
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
