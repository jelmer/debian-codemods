// Copyright (C) 2018-2025 Jelmer Vernooij <jelmer@debian.org>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

//! Functions for working with watch files.
//!
//! This module provides utilities for manipulating and fixing Debian watch files.
//!
//! Note: The Python version has more extensive functionality for discovering
//! watch file candidates from various sources (PyPI, CRAN, GitHub, etc.).
//! This Rust version currently focuses on the core watch file manipulation
//! functions. The candidate discovery functionality can be added later as needed.

/// Value assigned when fixing watch files
pub const WATCH_FIX_VALUE: i32 = 60;

/// Common pgpsigurlmangle patterns for signature files
pub const COMMON_PGPSIGURL_MANGLES: &[&str] = &[
    "s/$/.asc/",
    "s/$/.pgp/",
    "s/$/.gpg/",
    "s/$/.sig/",
    "s/$/.sign/",
];

// TODO: Port the following functions from Python when needed:
// - probe_signature: Try to find and verify signature files for releases
// - candidates_from_setup_py: Extract watch candidates from PyPI/setup.py
// - candidates_from_upstream_metadata: Extract watch candidates from debian/upstream/metadata
// - candidates_from_hackage: Extract watch candidates from Hackage
// - guess_github_watch_entry: Generate watch entry for GitHub repos
// - guess_launchpad_watch_entry: Generate watch entry for Launchpad projects
// - guess_cran_watch_entry: Generate watch entry for CRAN packages
// - find_candidates: Find all possible watch file candidates
// - fix_old_github_patterns: Fix deprecated GitHub URL patterns
// - fix_github_releases: Convert GitHub /releases to /tags
// - fix_watch_issues: Apply all known fixes to watch entries
// - verify_watch_entry: Verify a watch entry can discover expected versions
// - watch_entries_certainty: Calculate certainty for watch entries

/// Verify that a watch file can discover the current upstream version.
///
/// After modifying a watch file, call this function to verify that the watch entry
/// still works by running `discover_blocking()` and checking if the expected upstream
/// version appears among the discovered releases.
///
/// # Arguments
///
/// * `watch_path` - Path to the debian/watch file
/// * `package` - Source package name
/// * `upstream_version` - The upstream version string to look for
///
/// # Returns
///
/// * `Some(true)` - The upstream version was found among discovered releases
/// * `Some(false)` - Discovery succeeded but the upstream version was not found
/// * `None` - Discovery failed (e.g. network error, parse error)
pub fn verify_watch_entry_discovers_version(
    watch_path: &std::path::Path,
    package: &str,
    upstream_version: &str,
) -> Option<bool> {
    let content = std::fs::read_to_string(watch_path).ok()?;
    let watch_file = debian_watch::parse::parse(&content).ok()?;

    for entry in watch_file.entries() {
        let pkg = package.to_string();
        match entry.discover_blocking(|| pkg) {
            Ok(releases) => {
                if releases.iter().any(|r| r.version == upstream_version) {
                    return Some(true);
                }
            }
            Err(e) => {
                tracing::debug!("discover_blocking failed for watch entry: {}", e);
            }
        }
    }

    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_fix_value() {
        assert_eq!(WATCH_FIX_VALUE, 60);
    }

    #[test]
    fn test_common_pgpsigurl_mangles_contains_standard_extensions() {
        // Verify the most common signature file extensions are included
        assert!(COMMON_PGPSIGURL_MANGLES.contains(&"s/$/.asc/"));
        assert!(COMMON_PGPSIGURL_MANGLES.contains(&"s/$/.sig/"));
    }
}
