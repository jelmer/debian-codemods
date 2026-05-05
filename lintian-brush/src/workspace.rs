//! `FixerWorkspace`: an abstraction over the on-disk or in-editor state of a
//! Debian source package.
//!
//! Fixers historically reached into the working tree directly via
//! `std::fs`. That ties them to a particular host (the lintian-brush CLI,
//! which writes the tree to disk before invoking fixers). The
//! `FixerWorkspace` trait abstracts that access so the same fixer code can
//! also run inside an editor host (debian-lsp), where the source of truth for
//! a file is the open buffer rather than the path on disk.
//!
//! Two implementations are intended:
//!
//! * [`TreeFixerWorkspace`] — pure-`std` shim that operates on a base
//!   directory on disk. Used by the lintian-brush CLI; preserves the
//!   existing semantics where the harness writes the tree to disk, the
//!   fixer mutates files there, and the harness diffs the result.
//! * `LspFixerWorkspace` (lives in debian-lsp) — wraps a salsa-backed
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

use crate::{FixerError, LintianIssue, Version};

/// An editor handle for a single file in a [`FixerWorkspace`].
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
    fn commit(self: Box<Self>) -> Result<(), FixerError>;
}

/// Access to a Debian source package, as seen by a fixer.
///
/// Each typed accessor returns an editor for a well-known file. Callers can
/// also reach less-common files via [`read_file`](Self::read_file) /
/// [`write_file`](Self::write_file).
pub trait FixerWorkspace {
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
    /// Returns `Err(FixerError::NoChanges)` if the file is missing —
    /// detectors typically want that exact response.
    ///
    /// Implementations may cache the parse; the returned value is owned
    /// (`Control` is cheap to clone — its rowan green nodes are shared
    /// internally).
    fn parsed_control(&self) -> Result<Control, FixerError>;

    /// Read `debian/changelog` and return a parsed value.
    ///
    /// Returns `Err(FixerError::NoChanges)` if the file is missing.
    fn parsed_changelog(&self) -> Result<ChangeLog, FixerError>;

    /// Open `debian/control` for editing.
    ///
    /// Takes `&self` so that fixers can hold an editor and still call
    /// other workspace methods (`should_fix`, `read_file`, …). Implementations
    /// that need to record edits on the workspace itself should use interior
    /// mutability.
    ///
    /// Detectors don't need this — they emit `Action`s for the appliers to
    /// run. Use [`parsed_control`](Self::parsed_control) instead.
    fn control(&self) -> Result<Box<dyn Editor<Control> + '_>, FixerError>;

    /// Open `debian/changelog` for editing. See [`control`](Self::control).
    fn changelog(&self) -> Result<Box<dyn Editor<ChangeLog> + '_>, FixerError>;

    /// Read raw bytes of an arbitrary file relative to the package root.
    ///
    /// Returns `Ok(None)` if the file does not exist.
    fn read_file(&self, rel: &Path) -> Result<Option<Vec<u8>>, FixerError>;

    /// Write raw bytes to an arbitrary file relative to the package root.
    ///
    /// Creates the file if it does not exist.
    fn write_file(&self, rel: &Path, content: &[u8]) -> Result<(), FixerError>;

    /// Whether the given lintian issue should be fixed in this workspace,
    /// after taking lintian-overrides into account.
    fn should_fix(&self, issue: &LintianIssue) -> bool;
}

/// A [`FixerWorkspace`] backed by a directory on disk.
///
/// This is the implementation used by the `lintian-brush` CLI. The CLI's
/// fixer harness materialises the working tree to disk before invoking a
/// fixer; this workspace then operates on that directory, and breezyshim
/// picks up the resulting changes outside the fixer.
///
/// It contains no `breezyshim` types and so is safe to depend on from hosts
/// that don't want a Python runtime.
pub struct TreeFixerWorkspace {
    base_path: PathBuf,
    package: String,
    version: Version,
}

impl TreeFixerWorkspace {
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
    fn flush(&mut self) -> Result<(), FixerError> {
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
    fn commit(mut self: Box<Self>) -> Result<(), FixerError> {
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
                "FixerWorkspace Editor for {} dropped without commit; \
                 changes (if any) discarded",
                self.path.display()
            );
        }
    }
}

impl FixerWorkspace for TreeFixerWorkspace {
    fn package(&self) -> Option<&str> {
        Some(&self.package)
    }

    fn current_version(&self) -> Option<&Version> {
        Some(&self.version)
    }

    fn parsed_control(&self) -> Result<Control, FixerError> {
        let path = self.full_path(Path::new("debian/control"));
        let text = fs::read_to_string(&path).map_err(map_open_error)?;
        text.parse().map_err(|e: deb822_lossless::ParseError| {
            FixerError::Other(format!("Failed to parse {}: {}", path.display(), e))
        })
    }

    fn parsed_changelog(&self) -> Result<ChangeLog, FixerError> {
        let path = self.full_path(Path::new("debian/changelog"));
        let text = fs::read_to_string(&path).map_err(map_open_error)?;
        ChangeLog::read_relaxed(text.as_bytes())
            .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", path.display(), e)))
    }

    fn control(&self) -> Result<Box<dyn Editor<Control> + '_>, FixerError> {
        let path = self.full_path(Path::new("debian/control"));
        let original = fs::read_to_string(&path).map_err(map_open_error)?;
        let parsed: Control = original.parse().map_err(|e: deb822_lossless::ParseError| {
            FixerError::Other(format!("Failed to parse {}: {}", path.display(), e))
        })?;
        Ok(Box::new(FsEditor {
            parsed,
            original,
            path,
            committed: false,
        }))
    }

    fn changelog(&self) -> Result<Box<dyn Editor<ChangeLog> + '_>, FixerError> {
        let path = self.full_path(Path::new("debian/changelog"));
        let original = fs::read_to_string(&path).map_err(map_open_error)?;
        let parsed = ChangeLog::read_relaxed(original.as_bytes())
            .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", path.display(), e)))?;
        Ok(Box::new(FsEditor {
            parsed,
            original,
            path,
            committed: false,
        }))
    }

    fn read_file(&self, rel: &Path) -> Result<Option<Vec<u8>>, FixerError> {
        let path = self.full_path(rel);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(FixerError::Io(e)),
        }
    }

    fn write_file(&self, rel: &Path, content: &[u8]) -> Result<(), FixerError> {
        let path = self.full_path(rel);
        fs::write(&path, content)?;
        Ok(())
    }

    fn should_fix(&self, issue: &LintianIssue) -> bool {
        issue.should_fix(&self.base_path)
    }
}

/// A detector reads a Debian source package and emits
/// [`Diagnostic`](crate::diagnostic::Diagnostic)s describing what (if
/// anything) needs fixing, together with the [`Action`](crate::diagnostic::Action)s
/// that would fix it. Detectors do *not* mutate the tree.
///
/// This is the modern replacement for [`crate::builtin_fixers::BuiltinFixer`]'s
/// `diagnostics()` method. It carries no `basedir`/`package`/`current_version`
/// arguments — those are reachable through the workspace — and so works
/// unchanged in an LSP host that has no on-disk basedir for the open buffer.
///
/// Each detector is also wrapped in a [`crate::builtin_fixers::BuiltinFixer`]
/// adapter at registration time so the lintian-brush CLI driver picks it
/// up alongside the legacy `BuiltinFixer` fixers.
pub trait Detector: Send + Sync {
    /// Stable name of the detector. Matches the corresponding fixer name.
    fn name(&self) -> &'static str;

    /// Lintian tags this detector's diagnostics correspond to.
    fn lintian_tags(&self) -> &'static [&'static str];

    /// Detect issues in `ws` and return one [`Diagnostic`] per issue.
    ///
    /// `Ok(vec![])` means "nothing to fix, no error". `Err(NoChanges)` is
    /// also legal (and meaningfully equivalent) — detectors that compute
    /// "nothing to do" lazily often find that shape easier.
    fn detect(
        &self,
        ws: &dyn FixerWorkspace,
        preferences: &crate::FixerPreferences,
    ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError>;

    /// Optional: customise the description used in the resulting
    /// [`crate::FixerResult`]. Defaults to
    /// [`crate::builtin_fixers::default_describe`].
    fn describe(
        &self,
        fixed: &[crate::diagnostic::Diagnostic],
        actions: &[crate::diagnostic::Action],
    ) -> String {
        crate::builtin_fixers::default_describe(fixed, actions)
    }
}

/// Inventory entry for a [`Detector`].
///
/// Submitted automatically by [`declare_detector!`]; iterated via
/// [`iter_detectors`].
pub struct DetectorRegistration {
    /// Stable name of the detector.
    pub name: &'static str,
    /// Lintian tags this detector addresses.
    pub lintian_tags: &'static [&'static str],
    /// Constructor for an instance.
    pub create: fn() -> Box<dyn Detector>,
    /// Detectors that must run before this one.
    pub after: &'static [&'static str],
    /// Detectors that must run after this one.
    pub before: &'static [&'static str],
}

inventory::collect!(DetectorRegistration);

/// Iterate every registered [`Detector`].
pub fn iter_detectors() -> impl Iterator<Item = Box<dyn Detector>> {
    inventory::iter::<DetectorRegistration>
        .into_iter()
        .map(|reg| (reg.create)())
}

/// Bridge a [`Detector`] into the legacy [`crate::builtin_fixers::BuiltinFixer`]
/// trait so the CLI driver picks it up via
/// [`crate::builtin_fixers::get_builtin_fixers`].
///
/// `BuiltinFixer::diagnostics`'s default takes a `basedir` — we wrap it in
/// a [`TreeFixerWorkspace`] and call the underlying detector. The default
/// `BuiltinFixer::apply` then runs the actions through `appliers::apply_actions`
/// with the same basedir, so the on-disk write path is unchanged.
pub struct DetectorAdapter {
    detector: Box<dyn Detector>,
    name: &'static str,
    lintian_tags: &'static [&'static str],
}

impl DetectorAdapter {
    /// Wrap a [`Detector`] for use as a [`crate::builtin_fixers::BuiltinFixer`].
    pub fn new(detector: Box<dyn Detector>) -> Self {
        let name = detector.name();
        let lintian_tags = detector.lintian_tags();
        Self {
            detector,
            name,
            lintian_tags,
        }
    }
}

impl crate::builtin_fixers::BuiltinFixer for DetectorAdapter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn lintian_tags(&self) -> &'static [&'static str] {
        self.lintian_tags
    }

    fn diagnostics(
        &self,
        basedir: &Path,
        package: &str,
        current_version: &Version,
        preferences: &crate::FixerPreferences,
    ) -> Result<Vec<crate::diagnostic::Diagnostic>, FixerError> {
        let ws = TreeFixerWorkspace::new(basedir, package, current_version.clone());
        self.detector.detect(&ws, preferences)
    }

    fn describe(
        &self,
        fixed: &[crate::diagnostic::Diagnostic],
        actions: &[crate::diagnostic::Action],
    ) -> String {
        self.detector.describe(fixed, actions)
    }
}

/// Declare a [`Detector`] and register it.
///
/// Generates the `Detector` impl and an inventory submission that the CLI
/// driver picks up via the [`DetectorAdapter`].
///
/// # Example
///
/// ```ignore
/// declare_detector! {
///     name: "homepage-field-uses-insecure-uri",
///     tags: ["homepage-field-uses-insecure-uri"],
///     detect: |ws, prefs| detect(ws, prefs),
/// }
/// ```
///
/// The `after`, `before` and `describe` clauses are optional. `describe`
/// takes `fn(&[Diagnostic], &[Action]) -> String`.
#[macro_export]
macro_rules! declare_detector {
    (
        name: $name:expr,
        tags: [$($tag:expr),* $(,)?],
        $(after: [$($after:expr),* $(,)?],)?
        $(before: [$($before:expr),* $(,)?],)?
        detect: $detect_fn:expr
        $(, describe: $describe_fn:expr)?
        $(,)?
    ) => {
        struct DetectorImpl;

        impl $crate::workspace::Detector for DetectorImpl {
            fn name(&self) -> &'static str { $name }
            fn lintian_tags(&self) -> &'static [&'static str] { &[$($tag),*] }

            fn detect(
                &self,
                ws: &dyn $crate::workspace::FixerWorkspace,
                preferences: &$crate::FixerPreferences,
            ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> {
                let detect_fn: fn(
                    &dyn $crate::workspace::FixerWorkspace,
                    &$crate::FixerPreferences,
                ) -> Result<Vec<$crate::diagnostic::Diagnostic>, $crate::FixerError> = $detect_fn;
                detect_fn(ws, preferences)
            }

            $(
            fn describe(
                &self,
                fixed: &[$crate::diagnostic::Diagnostic],
                actions: &[$crate::diagnostic::Action],
            ) -> String {
                let describe_fn: fn(
                    &[$crate::diagnostic::Diagnostic],
                    &[$crate::diagnostic::Action],
                ) -> String = $describe_fn;
                describe_fn(fixed, actions)
            }
            )?
        }

        $crate::inventory::submit! {
            $crate::workspace::DetectorRegistration {
                name: $name,
                lintian_tags: &[$($tag),*],
                create: || Box::new(DetectorImpl),
                after: &[$($($after),*)?],
                before: &[$($($before),*)?],
            }
        }
    };
}

/// Map a file-open error: report missing required files as `NoChanges` so
/// that fixers can keep their familiar "file isn't there → bail out" idiom
/// without spelling the check out themselves.
fn map_open_error(e: io::Error) -> FixerError {
    if e.kind() == io::ErrorKind::NotFound {
        FixerError::NoChanges
    } else {
        FixerError::Io(e)
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

        let ws = TreeFixerWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());

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

        let ws = TreeFixerWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());

        let p = Path::new("debian/control");
        let bytes = ws.read_file(p).unwrap().unwrap();
        assert!(bytes.starts_with(b"Source: foo"));

        ws.write_file(Path::new("debian/x"), b"hello").unwrap();
        let back = ws.read_file(Path::new("debian/x")).unwrap().unwrap();
        assert_eq!(back, b"hello");

        assert!(ws.read_file(Path::new("debian/missing")).unwrap().is_none());
    }

    #[test]
    fn tree_workspace_missing_control_is_no_changes() {
        let tmp = TempDir::new().unwrap();
        // Don't make_pkg — no debian/ at all.
        let ws = TreeFixerWorkspace::new(tmp.path(), "foo", Version::from_str("1.0").unwrap());
        assert!(matches!(ws.control(), Err(FixerError::NoChanges)));
    }
}
