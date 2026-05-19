//! Utilities for reading and writing a Debian package working tree.

#![deny(missing_docs)]

/// Actions that describe changes to apply to a working tree.
pub mod action;

/// Apply [`action::Action`]s to a working tree.
pub mod appliers;

/// Operate on a Debian package.
pub mod workspace;

/// A workspace implementation on a filesystem.
pub mod fs_workspace;

/// Allow notification on certain events.
pub mod trigger;

/// Heuristics for classifying binary packages (transitional/meta-package).
pub mod package_class;

pub use debversion::Version;
pub use trigger::{ChangelogAspect, Trigger, WatchAspect};
pub use workspace::{Workspace, compat_level};

/// Errors that can occur while reading or writing workspace files.
#[derive(Debug)]
pub enum Error {
    /// The file is missing; the caller should treat this as "nothing to do".
    NotFound,
    /// An I/O error other than file-not-found.
    Io(std::io::Error),
    /// The file could not be parsed.
    Parse(String),
    /// Any other error.
    Other(String),
    /// A required external tool is not installed.
    MissingDependency(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "file not found"),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Parse(msg) => write!(f, "parse error: {}", msg),
            Error::Other(msg) => write!(f, "{}", msg),
            Error::MissingDependency(dep) => write!(f, "missing dependency: {}", dep),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<debian_analyzer::editor::EditorError> for Error {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        match e {
            debian_analyzer::editor::EditorError::IoError(e) => Error::Io(e),
            debian_analyzer::editor::EditorError::BrzError(e) => Error::Other(e.to_string()),
            debian_analyzer::editor::EditorError::GeneratedFile(p, _) => {
                Error::Other(format!("generated file: {}", p.display()))
            }
            debian_analyzer::editor::EditorError::FormattingUnpreservable(p, _) => {
                Error::Other(format!("formatting unpreservable: {}", p.display()))
            }
            debian_analyzer::editor::EditorError::TemplateError(p, _) => {
                Error::Other(format!("template error in {}", p.display()))
            }
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound
        } else {
            Error::Io(e)
        }
    }
}
