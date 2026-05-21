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
//! * [`FsWorkspace`] — pure-`std` shim that operates on a base
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

use std::path::{Path, PathBuf};

use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use debian_watch::parse::ParsedWatchFile;
use dep3::lossless::PatchHeader;
use makefile_lossless::Makefile;
use patchkit::edit::Patch;
use patchkit::quilt::Series;
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
    /// Parsing is relaxed: syntax errors are tolerated and the resulting
    /// AST may have missing or partially-recovered nodes. Detectors that
    /// need to reject malformed input should validate the structure they
    /// care about (e.g. that the source paragraph or a particular field
    /// exists) rather than expecting `Err`.
    ///
    /// Implementations may cache the parse; the returned value is owned
    /// (`Control` is cheap to clone — its rowan green nodes are shared
    /// internally).
    fn parsed_control(&self) -> Result<Control, Error>;

    /// Read `debian/changelog` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing. Parsing is
    /// relaxed; see [`parsed_control`](Self::parsed_control) for details
    /// on what that means.
    fn parsed_changelog(&self) -> Result<ChangeLog, Error>;

    /// Read `debian/copyright` and return a parsed value.
    ///
    /// Returns `Err(Error::NotFound)` if the file is missing, and
    /// `Err(Error::Parse)` only when the file isn't a machine-readable
    /// DEP-5 document at all (i.e. doesn't start with `Format:`).
    /// Parsing is otherwise relaxed; see
    /// [`parsed_control`](Self::parsed_control) for details on what that
    /// means.
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

    /// Read and parse `debian/patches/series`, the quilt patch series.
    ///
    /// Returns `Ok(None)` if the file does not exist (the package ships
    /// no quilt patches). Returns `Err` only if the file exists but
    /// cannot be read as a series.
    fn parsed_patches_series(&self) -> Result<Option<Series>, Error> {
        let rel = Path::new("debian/patches/series");
        match self.read_file(rel)? {
            None => Ok(None),
            Some(bytes) => {
                let series = Series::read(&bytes[..]).map_err(Error::Io)?;
                Ok(Some(series))
            }
        }
    }

    /// Read a quilt patch file and return its parsed DEP-3 header
    /// together with the parsed diff.
    ///
    /// `rel` is the patch's path relative to the package root (e.g.
    /// `debian/patches/fix-foo.patch`), as obtained by joining
    /// `debian/patches` with a name from [`parsed_patches_series`].
    ///
    /// Returns `Ok(None)` when the file does not exist. On success the
    /// tuple's first element is the patch's DEP-3 header, or `None` when
    /// the patch carries no header (a bare diff) or its header does not
    /// parse — the header is optional metadata. The second element is
    /// the lossless parse of the diff body; that parser is
    /// error-recovering, so a [`Patch`] is produced even for a malformed
    /// diff.
    ///
    /// Returns `Err(Error::Parse)` if the file exists but is not valid
    /// UTF-8.
    ///
    /// [`parsed_patches_series`]: Self::parsed_patches_series
    fn parsed_patch(&self, rel: &Path) -> Result<Option<(Option<PatchHeader>, Patch)>, Error> {
        let Some(bytes) = self.read_file(rel)? else {
            return Ok(None);
        };
        let content = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Parse(format!("{} is not valid UTF-8: {}", rel.display(), e)))?;
        let header_end = dep3::lossless::header_end(content);
        let header_text = &content[..header_end];
        let header = if header_text.trim().is_empty() {
            None
        } else {
            header_text.parse::<PatchHeader>().ok()
        };
        let patch = patchkit::edit::parse(&content[header_end..]).tree();
        Ok(Some((header, patch)))
    }

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
    /// Returns `Some` for the lintian-brush CLI ([`FsWorkspace`])
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
