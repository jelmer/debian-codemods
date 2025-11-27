//! VCS URL manipulation utilities.

use upstream_ontologist::vcs::{split_vcs_url, unsplit_vcs_url, VcsLocation};

/// Attempt to fix up broken Git URLs.
///
/// This function wraps upstream_ontologist's fixup_git_url functionality
/// and applies it to VCS URLs (which may include branch and subpath information).
pub fn fixup_broken_git_url(url: &str) -> String {
    let (repo_url, branch, subpath) = split_vcs_url(url);

    // Use tokio runtime to call the async fixup function
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return url.to_string(),
    };

    let fixed_repo_url = rt.block_on(upstream_ontologist::vcs::fixup_git_url(&repo_url));

    // If the repo URL changed or we have branch/subpath, reconstruct
    if fixed_repo_url != repo_url || branch.is_some() || subpath.is_some() {
        let parsed_url = match url::Url::parse(&fixed_repo_url) {
            Ok(u) => u,
            Err(_) => return url.to_string(),
        };

        let location = VcsLocation {
            url: parsed_url,
            branch,
            subpath,
        };

        unsplit_vcs_url(&location)
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixup_extra_colon() {
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_broken_git_url("https://github.com:jelmer/dulwich")
        );
        assert_eq!(
            "https://github.com/jelmer/dulwich -b blah",
            fixup_broken_git_url("https://github.com:jelmer/dulwich -b blah")
        );
    }

    #[test]
    fn test_git_to_https() {
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_broken_git_url("git://github.com/jelmer/dulwich")
        );
    }

    #[test]
    fn test_preserves() {
        assert_eq!(
            "https://github.com/jelmer/dulwich",
            fixup_broken_git_url("https://github.com/jelmer/dulwich")
        );
    }

    #[test]
    fn test_salsa_not_https() {
        assert_eq!(
            "https://salsa.debian.org/jelmer/dulwich",
            fixup_broken_git_url("git://salsa.debian.org/jelmer/dulwich")
        );
    }

    #[test]
    fn test_salsa_uses_cgit() {
        assert_eq!(
            "https://salsa.debian.org/jelmer/dulwich",
            fixup_broken_git_url("https://salsa.debian.org/cgit/jelmer/dulwich")
        );
    }

    #[test]
    fn test_strip_extra_slash() {
        assert_eq!(
            "https://salsa.debian.org/salve/auctex.git",
            fixup_broken_git_url("https://salsa.debian.org//salve/auctex.git")
        );
    }

    #[test]
    fn test_strip_extra_colon() {
        assert_eq!(
            "https://salsa.debian.org/mckinstry/lcov.git",
            fixup_broken_git_url("https://salsa.debian.org:/mckinstry/lcov.git")
        );
    }

    #[test]
    fn test_strip_username() {
        assert_eq!(
            "https://github.com/RPi-Distro/pgzero.git",
            fixup_broken_git_url("git://git@github.com:RPi-Distro/pgzero.git")
        );
        assert_eq!(
            "https://salsa.debian.org/debian-astro-team/pyavm.git",
            fixup_broken_git_url("https://git@salsa.debian.org:debian-astro-team/pyavm.git")
        );
    }

    #[test]
    fn test_freedesktop() {
        assert_eq!(
            "https://gitlab.freedesktop.org/xorg/xserver",
            fixup_broken_git_url("git://anongit.freedesktop.org/xorg/xserver")
        );
        assert_eq!(
            "https://gitlab.freedesktop.org/xorg/lib/libSM",
            fixup_broken_git_url("git://anongit.freedesktop.org/git/xorg/lib/libSM")
        );
    }

    #[test]
    fn test_anongit() {
        assert_eq!(
            "https://anongit.kde.org/kdev-php.git",
            fixup_broken_git_url("git://anongit.kde.org/kdev-php.git")
        );
    }

    #[test]
    fn test_gnome() {
        assert_eq!(
            "https://gitlab.gnome.org/GNOME/alacarte",
            fixup_broken_git_url("https://git.gnome.org/browse/alacarte")
        );
    }
}
