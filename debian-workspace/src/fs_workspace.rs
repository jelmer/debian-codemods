/// A `Workspace` implementation that operates on a directory on disk.
use crate::Error;
use crate::Version;
use crate::workspace::{Editor, Workspace};
use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use debian_watch::parse::ParsedWatchFile;
use makefile_lossless::Makefile;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

/// A [`Workspace`] backed by a directory on disk.
///
/// This is the implementation used by the `lintian-brush` CLI. The CLI's
/// fixer harness materialises the working tree to disk before invoking a
/// fixer; this workspace then operates on that directory, and breezyshim
/// picks up the resulting changes outside the fixer.
///
/// It contains no `breezyshim` types and so is safe to depend on from hosts
/// that don't want a Python runtime.
pub struct FsWorkspace {
    base_path: PathBuf,
    package: Option<String>,
    version: Option<Version>,
}

impl FsWorkspace {
    /// Create a new tree-backed workspace.
    ///
    /// * `base_path` — absolute filesystem path of the package root (the
    ///   directory containing `debian/`).
    /// * `package`, `version` — taken from `debian/changelog` by the caller.
    ///   Pass `None` when the caller hasn't read the changelog (e.g. tests, or
    ///   tools that don't surface package metadata to their detectors).
    pub fn new(
        base_path: impl Into<PathBuf>,
        package: Option<String>,
        version: Option<Version>,
    ) -> Self {
        Self {
            base_path: base_path.into(),
            package,
            version,
        }
    }

    /// The absolute on-disk root of the package.
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    fn full_path(&self, rel: &Path) -> PathBuf {
        self.base_path.join(rel)
    }
}

/// `Editor` impl for a parsed file backed by a path on disk.
///
/// Holds the parsed value, its original on-disk text (so we can detect
/// changes), and the absolute path to write back to. Serialisation goes
/// through the type's `Display` impl.
struct FsEditor<T> {
    parsed: T,
    original: String,
    path: PathBuf,
    committed: bool,
}

impl<T> std::ops::Deref for FsEditor<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.parsed
    }
}

impl<T> std::ops::DerefMut for FsEditor<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.parsed
    }
}

impl<T: std::fmt::Display> FsEditor<T> {
    fn flush(&mut self) -> Result<(), Error> {
        if self.committed {
            return Ok(());
        }
        let new_text = self.parsed.to_string();
        if new_text != self.original {
            fs::write(&self.path, &new_text)?;
        }
        self.committed = true;
        Ok(())
    }
}

impl<T: std::fmt::Display + 'static> Editor<T> for FsEditor<T> {
    fn commit(mut self: Box<Self>) -> Result<(), Error> {
        self.flush()
    }
}

impl<T> Drop for FsEditor<T> {
    fn drop(&mut self) {
        // Tree-mode fixers traditionally rely on implicit write-back. We
        // *don't* attempt it here because we'd have no way to surface a
        // serialisation failure: the tests would silently lose data. Callers
        // must invoke `commit` explicitly. If they forgot, log loudly.
        if !self.committed {
            tracing::warn!(
                "Workspace Editor for {} dropped without commit; \
                 changes (if any) discarded",
                self.path.display()
            );
        }
    }
}

impl Workspace for FsWorkspace {
    fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    fn current_version(&self) -> Option<&Version> {
        self.version.as_ref()
    }

    fn parsed_control(&self) -> Result<Control, Error> {
        let path = self.full_path(Path::new("debian/control"));
        let text = fs::read_to_string(&path)?;
        let (control, errors) = Control::read_relaxed(text.as_bytes())
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))?;
        if !errors.is_empty() {
            tracing::debug!(
                "{} has {} parse warning(s): {}",
                path.display(),
                errors.len(),
                errors.join("; ")
            );
        }
        Ok(control)
    }

    fn parsed_changelog(&self) -> Result<ChangeLog, Error> {
        let path = self.full_path(Path::new("debian/changelog"));
        let text = fs::read_to_string(&path)?;
        ChangeLog::read_relaxed(text.as_bytes())
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))
    }

    fn parsed_copyright(&self) -> Result<Copyright, Error> {
        let path = self.full_path(Path::new("debian/copyright"));
        let text = fs::read_to_string(&path)?;
        let (copyright, errors) = Copyright::from_str_relaxed(&text)
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {:?}", path.display(), e)))?;
        if !errors.is_empty() {
            tracing::debug!(
                "{} has {} parse warning(s): {}",
                path.display(),
                errors.len(),
                errors.join("; ")
            );
        }
        Ok(copyright)
    }

    fn parsed_upstream_metadata(&self) -> Result<yaml_edit::YamlFile, Error> {
        use std::str::FromStr;
        let path = self.full_path(Path::new("debian/upstream/metadata"));
        let text = fs::read_to_string(&path)?;
        yaml_edit::YamlFile::from_str(&text)
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))
    }

    fn parsed_watch(&self) -> Result<ParsedWatchFile, Error> {
        let path = self.full_path(Path::new("debian/watch"));
        let text = fs::read_to_string(&path)?;
        debian_watch::parse::parse(&text)
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {:?}", path.display(), e)))
    }

    fn parsed_rules(&self) -> Result<Makefile, Error> {
        let path = self.full_path(Path::new("debian/rules"));
        let bytes = fs::read(&path)?;
        Makefile::read_relaxed(bytes.as_slice())
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))
    }

    fn source_format(&self) -> Result<Option<String>, Error> {
        match self.read_file(Path::new("debian/source/format"))? {
            Some(b) => Ok(std::str::from_utf8(&b)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())),
            None => Ok(None),
        }
    }

    fn control(&self) -> Result<Box<dyn Editor<Control> + '_>, Error> {
        let path = self.full_path(Path::new("debian/control"));
        let original = fs::read_to_string(&path)?;
        let parsed: Control = original.parse().map_err(|e: deb822_lossless::ParseError| {
            Error::Parse(format!("Failed to parse {}: {}", path.display(), e))
        })?;
        Ok(Box::new(FsEditor {
            parsed,
            original,
            path,
            committed: false,
        }))
    }

    fn changelog(&self) -> Result<Box<dyn Editor<ChangeLog> + '_>, Error> {
        let path = self.full_path(Path::new("debian/changelog"));
        let original = fs::read_to_string(&path)?;
        let parsed = ChangeLog::read_relaxed(original.as_bytes())
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))?;
        Ok(Box::new(FsEditor {
            parsed,
            original,
            path,
            committed: false,
        }))
    }

    fn debcargo(&self) -> Result<Option<Box<dyn Editor<DocumentMut> + '_>>, Error> {
        let path = self.full_path(Path::new("debian/debcargo.toml"));
        let original = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::Io(e)),
        };
        let parsed: DocumentMut = original
            .parse()
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))?;
        Ok(Some(Box::new(FsEditor {
            parsed,
            original,
            path,
            committed: false,
        })))
    }

    fn read_file(&self, rel: &Path) -> Result<Option<std::borrow::Cow<'_, [u8]>>, Error> {
        let path = self.full_path(rel);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(std::borrow::Cow::Owned(bytes))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn write_file(&self, rel: &Path, content: &[u8]) -> Result<(), Error> {
        let path = self.full_path(rel);
        fs::write(&path, content)?;
        Ok(())
    }

    fn list_dir(&self, rel: &Path) -> Result<Option<Vec<String>>, Error> {
        let path = self.full_path(rel);
        let read_dir = match fs::read_dir(&path) {
            Ok(it) => it,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::Io(e)),
        };
        let mut names = Vec::new();
        for entry in read_dir {
            let entry = entry?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        Ok(Some(names))
    }

    fn walk_dir(&self, rel: &Path) -> Result<Option<Vec<PathBuf>>, Error> {
        let abs = self.full_path(rel);
        if !abs.exists() {
            return Ok(None);
        }
        let mut out = Vec::new();
        let mut stack: Vec<PathBuf> = vec![abs.clone()];
        while let Some(dir) = stack.pop() {
            let read_dir = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::Io(e)),
            };
            for entry in read_dir {
                let entry = entry?;
                let ft = entry.file_type()?;
                let path = entry.path();
                if ft.is_dir() {
                    stack.push(path);
                } else if ft.is_file() {
                    let rel_path = path
                        .strip_prefix(&self.base_path)
                        .map(|p| p.to_path_buf())
                        .unwrap_or(path);
                    out.push(rel_path);
                }
                // Skip symlinks and other non-regular entries.
            }
        }
        Ok(Some(out))
    }

    fn file_mode(&self, rel: &Path) -> Result<Option<u32>, Error> {
        use std::os::unix::fs::PermissionsExt;
        let path = self.full_path(rel);
        match fs::metadata(&path) {
            Ok(m) => Ok(Some(m.permissions().mode())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn base_path(&self) -> Option<&Path> {
        Some(&self.base_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn make_pkg(dir: &Path) {
        let debian = dir.join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: foo\nDescription: bar\n bar\n",
        )
        .unwrap();
        fs::write(
            debian.join("changelog"),
            "foo (1.0) unstable; urgency=medium\n\n  * Initial.\n\n -- A B <a@b>  Mon, 01 Jan 2024 00:00:00 +0000\n",
        )
        .unwrap();
    }

    #[test]
    fn tree_workspace_reads_and_writes_control() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());

        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );

        {
            let control = ws.control().unwrap();
            let mut source = control.source().unwrap();
            source.set_homepage(&url::Url::parse("https://example.com/").unwrap());
            control.commit().unwrap();
        }

        let on_disk = fs::read_to_string(tmp.path().join("debian/control")).unwrap();
        assert!(on_disk.contains("Homepage: https://example.com/"));
    }

    #[test]
    fn tree_workspace_read_write_raw_file() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());

        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );

        let p = Path::new("debian/control");
        let bytes = ws.read_file(p).unwrap().unwrap();
        assert!(bytes.starts_with(b"Source: foo"));

        ws.write_file(Path::new("debian/x"), b"hello").unwrap();
        let back = ws.read_file(Path::new("debian/x")).unwrap().unwrap();
        assert_eq!(&*back, b"hello");

        assert!(ws.read_file(Path::new("debian/missing")).unwrap().is_none());
    }

    #[test]
    fn tree_workspace_missing_control_is_not_found() {
        let tmp = TempDir::new().unwrap();
        // Don't make_pkg — no debian/ at all.
        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );
        assert!(matches!(ws.control(), Err(Error::NotFound)));
    }

    #[test]
    fn tree_workspace_walk_dir_returns_relative_files() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        // Add a subdirectory with a file to verify recursion.
        let nested = tmp.path().join("debian/source");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("format"), "3.0 (quilt)\n").unwrap();

        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );
        let mut paths = ws.walk_dir(Path::new("debian")).unwrap().unwrap();
        paths.sort();

        assert_eq!(
            paths,
            vec![
                PathBuf::from("debian/changelog"),
                PathBuf::from("debian/control"),
                PathBuf::from("debian/source/format"),
            ]
        );
    }

    #[test]
    fn tree_workspace_walk_dir_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );
        assert!(ws.walk_dir(Path::new("debian")).unwrap().is_none());
    }

    #[test]
    fn debcargo_absent_returns_none() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );
        assert!(ws.parsed_debcargo().unwrap().is_none());
        assert!(ws.debcargo().unwrap().is_none());
    }

    #[test]
    fn debcargo_read_and_write() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let toml = "[source]\nvcs_git = \"https://salsa.debian.org/rust-team/debcargo-conf\"\n";
        fs::write(tmp.path().join("debian/debcargo.toml"), toml).unwrap();

        let ws = FsWorkspace::new(
            tmp.path(),
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        );

        let doc = ws.parsed_debcargo().unwrap().unwrap();
        assert_eq!(
            doc["source"]["vcs_git"].as_str().unwrap(),
            "https://salsa.debian.org/rust-team/debcargo-conf"
        );

        {
            let mut editor = ws.debcargo().unwrap().unwrap();
            editor["source"]["vcs_git"] =
                toml_edit::value("https://salsa.debian.org/rust-team/debcargo-conf.git");
            editor.commit().unwrap();
        }

        let on_disk = fs::read_to_string(tmp.path().join("debian/debcargo.toml")).unwrap();
        assert_eq!(
            on_disk,
            "[source]\nvcs_git = \"https://salsa.debian.org/rust-team/debcargo-conf.git\"\n"
        );
    }

    fn workspace(dir: &Path) -> FsWorkspace {
        FsWorkspace::new(
            dir,
            Some("foo".into()),
            Some(Version::from_str("1.0").unwrap()),
        )
    }

    #[test]
    fn patches_series_absent_returns_none() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        assert!(
            workspace(tmp.path())
                .parsed_patches_series()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn patches_series_parsed() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(patches.join("series"), "one.patch\ntwo.patch\n").unwrap();

        let series = workspace(tmp.path())
            .parsed_patches_series()
            .unwrap()
            .unwrap();
        assert_eq!(series.entries.len(), 2);
    }

    #[test]
    fn patch_absent_returns_none() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        assert!(
            workspace(tmp.path())
                .parsed_patch(Path::new("debian/patches/missing.patch"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn patch_header_and_diff_parsed() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(
            patches.join("fix.patch"),
            "Description: Fix a typo\nLast-Update: 2024-01-02\n\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-teh\n+the\n",
        )
        .unwrap();

        let (header, patch) = workspace(tmp.path())
            .parsed_patch(Path::new("debian/patches/fix.patch"))
            .unwrap()
            .unwrap();
        let header = header.expect("patch has a DEP-3 header");
        assert_eq!(
            header.as_deb822().get("Description").as_deref(),
            Some("Fix a typo")
        );
        assert_eq!(patch.patch_files().count(), 1);
    }

    #[test]
    fn patch_without_header_returns_none_header() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let patches = tmp.path().join("debian/patches");
        fs::create_dir_all(&patches).unwrap();
        fs::write(
            patches.join("bare.patch"),
            "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-teh\n+the\n",
        )
        .unwrap();

        let (header, patch) = workspace(tmp.path())
            .parsed_patch(Path::new("debian/patches/bare.patch"))
            .unwrap()
            .unwrap();
        assert!(header.is_none());
        assert_eq!(patch.patch_files().count(), 1);
    }
}
