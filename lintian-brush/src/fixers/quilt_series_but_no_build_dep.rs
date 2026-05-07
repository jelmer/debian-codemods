use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use std::path::{Path, PathBuf};

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.read_file(Path::new("debian/debcargo.toml"))?.is_some() {
        return Ok(Vec::new());
    }

    let format = match ws.read_file(Path::new("debian/source/format"))? {
        Some(b) => String::from_utf8(b)
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        None => String::new(),
    };
    if format == "3.0 (quilt)" {
        return Ok(Vec::new());
    }

    if ws.read_file(Path::new("debian/patches/series"))?.is_none() {
        return Ok(Vec::new());
    }

    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };

    // Skip if quilt is already in Build-Depends.
    if let Some(value) = source.as_deb822().get("Build-Depends") {
        let (relations, _errors) =
            debian_control::lossless::relations::Relations::parse_relaxed(&value, true);
        if relations.entries().any(|e| {
            e.relations()
                .any(|r| r.try_name().as_deref() == Some("quilt"))
        }) {
            return Ok(Vec::new());
        }
    }

    let issue = LintianIssue::source_with_info(
        "quilt-series-but-no-build-dep",
        vec!["[debian/patches/series]".to_string()],
    );

    Ok(vec![Diagnostic::with_actions(
        issue,
        "debian/patches/series exists but quilt is not a build-dependency.",
        "Add missing dependency on quilt.",
        vec![Action::Deb822(Deb822Action::EnsureRelation {
            file: control_rel,
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "quilt".into(),
        })],
    )])
}

declare_detector! {
    name: "quilt-series-but-no-build-dep",
    tags: ["quilt-series-but-no-build-dep"],
    triggers: [
        crate::workspace::Trigger::File("debian/debcargo.toml"),
        crate::workspace::Trigger::File("debian/source/format"),
        crate::workspace::Trigger::File("debian/patches/series"),
        crate::workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
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
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_simple() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        let control = debian.join("control");
        fs::write(
            &control,
            "Source: blah\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper\n\nPackage: blah\nDescription: blah blah\n Blah blah\n",
        )
        .unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&control).unwrap(),
            "Source: blah\nMaintainer: Joe Example <joe@example.com>\nBuild-Depends: debhelper, quilt\n\nPackage: blah\nDescription: blah blah\n Blah blah\n",
        );
    }

    #[test]
    fn test_no_change_when_quilt_already_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper, quilt\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_3_0_quilt() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        let patches = debian.join("patches");
        let source = debian.join("source");
        fs::create_dir_all(&patches).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(patches.join("series"), "01-foo.patch\n").unwrap();
        fs::write(source.join("format"), "3.0 (quilt)\n").unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_series() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: blah\nBuild-Depends: debhelper\n\nPackage: blah\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
