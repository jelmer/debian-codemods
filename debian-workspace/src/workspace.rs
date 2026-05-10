//! `Workspace`: an abstraction over the on-disk or in-editor state of a
//! Debian source package.
//!
//! Fixers historically reached into the working tree directly via
//! `std::fs`. That ties them to a particular host (the lintian-brush CLI,
//! which writes the tree to disk before invoking fixers). The
//! `Workspace` trait abstracts that access so the same fixer code can
//! also run inside an editor host (debian-lsp), where the source of truth for
//! a file is the open buffer rather than the path on disk.
//!
//! Two implementations are intended:
//!
//! * [`TreeWorkspace`] — pure-`std` shim that operates on a base
//!   directory on disk. Used by the lintian-brush CLI; preserves the
//!   existing semantics where the harness writes the tree to disk, the
//!   fixer mutates files there, and the harness diffs the result.
//! * `LspWorkspace` (lives in debian-lsp) — wraps a salsa-backed
//!   in-memory workspace. Mutations are accumulated as a single
//!   `WorkspaceEdit` rather than being written back to disk.
//!
//! The trait is deliberately `breezyshim`-free so that hosts that don't want
//! a Python runtime (notably debian-lsp) can depend on it without pulling in
//! PyO3.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use debian_watch::parse::ParsedWatchFile;
use makefile_lossless::Makefile;
use toml_edit::DocumentMut;

use crate::{Error, Version};

/// An editor handle for a single file in a [`Workspace`].
///
/// The parsed value is reachable via `Deref`/`DerefMut`; mutate it as you
/// would the bare type. Changes are persisted by calling
/// [`commit`](Self::commit). Dropping an editor without committing discards
/// the changes (and emits a warning) — explicit commit is required so that
/// serialisation failures can be reported.
///
/// `T` is the parsed representation (e.g.
/// [`debian_control::lossless::Control`]).
pub trait Editor<T>: std::ops::Deref<Target = T> + std::ops::DerefMut<Target = T> {
    /// Persist any modifications to the underlying workspace.
    ///
    /// For a tree-backed workspace this writes the file back to disk; for an
    /// editor-backed workspace it records a `TextEdit` against the buffer.
    /// Calling `commit` more than once is a no-op.
    fn commit(self: Box<Self>) -> Result<(), Error>;
}

/// Access to a Debian source package, as seen by a fixer.
///
/// Each typed accessor returns an editor for a well-known file. Callers can
/// also reach less-common files via [`read_file`](Self::read_file) /
/// [`write_file`](Self::write_file).
pub trait Workspace {
    /// The source package name, as read from `debian/changelog`.
    ///
    /// Returns `None` when the changelog is missing or unreadable. Hosts
    /// that legitimately don't have a changelog (e.g. an LSP that lost
    /// access to it) should return `None` rather than fabricating a name.
    fn package(&self) -> Option<&str>;

    /// The current version of the package, as read from `debian/changelog`.
    ///
    /// Returns `None` when the changelog is missing or unreadable.
    fn current_version(&self) -> Option<&Version>;

    /// Read `debian/control` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing —
    /// detectors typically want that exact response.
    ///
    /// Implementations may cache the parse; the returned value is owned
    /// (`Control` is cheap to clone — its rowan green nodes are shared
    /// internally).
    fn parsed_control(&self) -> Result<Control, Error>;

    /// Read `debian/changelog` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing.
    fn parsed_changelog(&self) -> Result<ChangeLog, Error>;

    /// Read `debian/copyright` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing.
    /// Returns the lossless `Copyright` even when the file isn't a
    /// machine-readable DEP-5 document — callers that care should check
    /// for a header paragraph.
    fn parsed_copyright(&self) -> Result<Copyright, Error>;

    /// Read `debian/upstream/metadata` and return its parsed YAML.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing or
    /// unparseable.
    fn parsed_upstream_metadata(&self) -> Result<yaml_edit::YamlFile, Error>;

    /// Read `debian/watch` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing.
    fn parsed_watch(&self) -> Result<ParsedWatchFile, Error>;

    /// Read `debian/rules` and return the parsed Makefile.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing. Uses
    /// `Makefile::read_relaxed`, mirroring the behaviour every fixer
    /// currently expects from `debian/rules` parsing.
    fn parsed_rules(&self) -> Result<Makefile, Error>;

    /// Read the trimmed contents of `debian/source/format`.
    ///
    /// Returns `Ok(None)` if the file is missing. The default format
    /// (`1.0`) is *not* substituted — callers see exactly what is on
    /// disk so they can distinguish "no file" from "explicit 1.0".
    fn source_format(&self) -> Result<Option<String>, Error>;

    /// Open `debian/control` for editing.
    ///
    /// Takes `&self` so that fixers can hold an editor and still call
    /// other workspace methods (`read_file`, …). Implementations
    /// that need to record edits on the workspace itself should use interior
    /// mutability.
    ///
    /// Detectors don't need this — they emit `Action`s for the appliers to
    /// run. Use [`parsed_control`](Self::parsed_control) instead.
    fn control(&self) -> Result<Box<dyn Editor<Control> + '_>, Error>;

    /// Open `debian/changelog` for editing. See [`control`](Self::control).
    fn changelog(&self) -> Result<Box<dyn Editor<ChangeLog> + '_>, Error>;

    /// Read `debian/debcargo.toml` and return a parsed TOML document.
    ///
    /// Returns `Ok(None)` if the file does not exist (package is not a
    /// debcargo-managed crate). Returns `Err` if the file exists but cannot
    /// be parsed.
    fn parsed_debcargo(&self) -> Result<Option<DocumentMut>, Error> {
        let rel = Path::new("debian/debcargo.toml");
        match self.read_file(rel)? {
            None => Ok(None),
            Some(bytes) => {
                let text = String::from_utf8(bytes.into_owned()).map_err(|e| {
                    Error::Parse(format!("debcargo.toml is not valid UTF-8: {}", e))
                })?;
                let doc: DocumentMut = text
                    .parse()
                    .map_err(|e| Error::Parse(format!("Failed to parse debcargo.toml: {}", e)))?;
                Ok(Some(doc))
            }
        }
    }

    /// Open `debian/debcargo.toml` for editing.
    ///
    /// Returns `Ok(None)` if the file does not exist.
    /// Returns `Err` if the file exists but cannot be parsed.
    fn debcargo(&self) -> Result<Option<Box<dyn Editor<DocumentMut> + '_>>, Error>;

    /// Read raw bytes of an arbitrary file relative to the package root.
    ///
    /// Returns `Ok(None)` if the file does not exist.
    ///
    /// The returned `Cow` is borrowed when the host has the bytes
    /// already in memory (an LSP host with the file open in an editor
    /// buffer) and owned when they had to be fetched (a disk read).
    /// Detectors that need owned bytes can call `.into_owned()`.
    fn read_file(&self, rel: &Path) -> Result<Option<std::borrow::Cow<'_, [u8]>>, Error>;

    /// Write raw bytes to an arbitrary file relative to the package root.
    ///
    /// Creates the file if it does not exist.
    fn write_file(&self, rel: &Path, content: &[u8]) -> Result<(), Error>;

    /// List the entries of a directory relative to the package root.
    ///
    /// Returns the file (and subdirectory) names within `rel`, without any
    /// path prefix. Returns `Ok(None)` if the directory does not exist.
    ///
    /// The order of returned entries is unspecified — a non-`Tree` host
    /// (an LSP) may not have a meaningful directory ordering.
    fn list_dir(&self, rel: &Path) -> Result<Option<Vec<String>>, Error>;

    /// Recursively walk `rel`, returning the relative paths of every
    /// regular file beneath it (paths are relative to the package root,
    /// not to `rel`).
    ///
    /// Symbolic links and other non-regular entries are skipped. Returns
    /// `Ok(None)` if `rel` does not exist.
    ///
    /// The order of returned paths is unspecified. Hosts that can't
    /// meaningfully walk a tree (e.g. an LSP that only knows about open
    /// buffers) may return only the files they currently track.
    fn walk_dir(&self, rel: &Path) -> Result<Option<Vec<PathBuf>>, Error> {
        // Default impl: depth-first walk via list_dir + read_file.
        // Hosts that have a faster path can override.
        let Some(top_entries) = self.list_dir(rel)? else {
            return Ok(None);
        };
        let mut out = Vec::new();
        let mut stack: Vec<(PathBuf, Vec<String>)> = vec![(rel.to_path_buf(), top_entries)];
        while let Some((dir, entries)) = stack.pop() {
            for name in entries {
                let child = dir.join(&name);
                match self.list_dir(&child)? {
                    Some(sub) => stack.push((child, sub)),
                    None => out.push(child),
                }
            }
        }
        Ok(Some(out))
    }

    /// Read the Unix file mode of `rel`, or `None` if the file is missing.
    ///
    /// Hosts that don't track a meaningful mode (e.g. an LSP serving an
    /// in-memory buffer) may return `Ok(None)` even when the file exists.
    /// Detectors that key off mode (e.g. checking that `debian/rules` is
    /// executable) treat that the same as "not present" and skip.
    fn file_mode(&self, rel: &Path) -> Result<Option<u32>, Error>;

    /// On-disk root for hosts that have one.
    ///
    /// Returns `Some` for the lintian-brush CLI ([`TreeWorkspace`])
    /// where the package has been materialised to disk. Returns `None`
    /// for in-memory hosts (an LSP serving open buffers); detectors that
    /// genuinely need to walk the source tree (e.g. an upstream-metadata
    /// guesser, a license scanner) should treat `None` as "skip — we
    /// can't help here".
    ///
    /// Prefer the typed accessors ([`read_file`](Self::read_file),
    /// [`list_dir`](Self::list_dir), …) wherever possible. Reach for
    /// this only when an external library insists on a `&Path` for the
    /// whole tree.
    fn base_path(&self) -> Option<&Path> {
        None
    }
}

/// A [`Workspace`] backed by a directory on disk.
///
/// This is the implementation used by the `lintian-brush` CLI. The CLI's
/// fixer harness materialises the working tree to disk before invoking a
/// fixer; this workspace then operates on that directory, and breezyshim
/// picks up the resulting changes outside the fixer.
///
/// It contains no `breezyshim` types and so is safe to depend on from hosts
/// that don't want a Python runtime.
pub struct TreeWorkspace {
    base_path: PathBuf,
    package: String,
    version: Version,
}

impl TreeWorkspace {
    /// Create a new tree-backed workspace.
    ///
    /// * `base_path` — absolute filesystem path of the package root (the
    ///   directory containing `debian/`).
    /// * `package`, `version` — taken from `debian/changelog` by the caller.
    pub fn new(
        base_path: impl Into<PathBuf>,
        package: impl Into<String>,
        version: Version,
    ) -> Self {
        Self {
            base_path: base_path.into(),
            package: package.into(),
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

impl Workspace for TreeWorkspace {
    fn package(&self) -> Option<&str> {
        Some(&self.package)
    }

    fn current_version(&self) -> Option<&Version> {
        Some(&self.version)
    }

    fn parsed_control(&self) -> Result<Control, Error> {
        let path = self.full_path(Path::new("debian/control"));
        let text = fs::read_to_string(&path)?;
        text.parse().map_err(|e: deb822_lossless::ParseError| {
            Error::Parse(format!("Failed to parse {}: {}", path.display(), e))
        })
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
        text.parse()
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {:?}", path.display(), e)))
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

/// Read the debhelper compat level from a workspace.
///
/// Looks at `debian/compat` first, then falls back to the `X-DH-Compat`
/// field or a `debhelper-compat` build dependency in `debian/control`.
/// Returns `Ok(None)` when neither source is present or parseable.
pub fn compat_level(ws: &dyn Workspace) -> Result<Option<u8>, Error> {
    if let Some(bytes) = ws.read_file(Path::new("debian/compat"))? {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let trimmed = text
                .split_once('#')
                .map_or(text, |(before, _)| before)
                .trim();
            if let Ok(level) = trimmed.parse::<u8>() {
                return Ok(Some(level));
            }
        }
    }

    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(Error::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let Some(source) = control.source() else {
        return Ok(None);
    };

    if let Some(dh_compat) = source.as_deb822().get("X-DH-Compat") {
        let trimmed = dh_compat
            .split_once('#')
            .map_or(dh_compat.as_str(), |(before, _)| before)
            .trim();
        if let Ok(level) = trimmed.parse::<u8>() {
            return Ok(Some(level));
        }
    }

    let Some(build_depends) = source.build_depends() else {
        return Ok(None);
    };
    let Some(rel) = build_depends
        .entries()
        .flat_map(|entry| entry.relations().collect::<Vec<_>>())
        .find(|r| r.try_name().as_deref() == Some("debhelper-compat"))
    else {
        return Ok(None);
    };
    Ok(rel
        .version()
        .and_then(|(_op, v)| v.to_string().parse::<u8>().ok()))
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

        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());

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

        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());

        let p = Path::new("debian/control");
        let bytes = ws.read_file(p).unwrap().unwrap();
        assert!(bytes.starts_with(b"Source: foo"));

        ws.write_file(Path::new("debian/x"), b"hello").unwrap();
        let back = ws.read_file(Path::new("debian/x")).unwrap().unwrap();
        assert_eq!(&*back, b"hello");

        assert!(ws.read_file(Path::new("debian/missing")).unwrap().is_none());
    }

    #[test]
    fn tree_workspace_missing_control_is_no_changes() {
        let tmp = TempDir::new().unwrap();
        // Don't make_pkg — no debian/ at all.
        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());
        assert!(matches!(ws.control(), Err(FixerError::NoChanges)));
    }

    #[test]
    fn tree_workspace_walk_dir_returns_relative_files() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        // Add a subdirectory with a file to verify recursion.
        let nested = tmp.path().join("debian/source");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("format"), "3.0 (quilt)\n").unwrap();

        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());
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
        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());
        assert!(ws.walk_dir(Path::new("debian")).unwrap().is_none());
    }

    #[test]
    fn debcargo_absent_returns_none() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());
        assert!(ws.parsed_debcargo().unwrap().is_none());
        assert!(ws.debcargo().unwrap().is_none());
    }

    #[test]
    fn debcargo_read_and_write() {
        let tmp = TempDir::new().unwrap();
        make_pkg(tmp.path());
        let toml = "[source]\nvcs_git = \"https://salsa.debian.org/rust-team/debcargo-conf\"\n";
        fs::write(tmp.path().join("debian/debcargo.toml"), toml).unwrap();

        let ws = TreeWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());

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
}
