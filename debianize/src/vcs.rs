use crate::Error;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::PyWorkingTree;
use debian_control::lossless::Control;
use debian_control::lossless::Source;
use std::collections::HashMap;
use std::path::Path;
use url::Url;

/// Error for when no VCS location is specified or can be determined
#[derive(Debug)]
pub struct NoVcsLocation;

impl std::fmt::Display for NoVcsLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No VCS location specified or determined")
    }
}

impl std::error::Error for NoVcsLocation {}

/// Error for when VCS fields are already specified
#[derive(Debug)]
pub struct VcsAlreadySpecified {
    pub vcs_type: String,
    pub url: String,
}

impl std::fmt::Display for VcsAlreadySpecified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Vcs is already specified: {} {}",
            self.vcs_type, self.url
        )
    }
}

impl std::error::Error for VcsAlreadySpecified {}

/// Mapping of maintainer emails to Salsa team names
fn get_maintainer_email_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("pkg-javascript-devel@lists.alioth.debian.org", "js-team");
    map.insert(
        "python-modules-team@lists.alioth.debian.org",
        "python-team/modules",
    );
    map.insert(
        "python-apps-team@lists.alioth.debian.org",
        "python-team/applications",
    );
    map.insert(
        "debian-science-maintainers@lists.alioth.debian.org",
        "science-team",
    );
    map.insert(
        "pkg-perl-maintainers@lists.alioth.debian.org",
        "perl-team/modules/packages",
    );
    map.insert("pkg-java-maintainers@lists.alioth.debian.org", "java-team");
    map.insert(
        "pkg-ruby-extras-maintainers@lists.alioth.debian.org",
        "ruby-team",
    );
    map.insert("pkg-clamav-devel@lists.alioth.debian.org", "clamav-team");
    map.insert(
        "pkg-go-maintainers@lists.alioth.debian.org",
        "go-team/packages",
    );
    map.insert("pkg-games-devel@lists.alioth.debian.org", "games-team");
    map.insert(
        "pkg-telepathy-maintainers@lists.alioth.debian.org",
        "telepathy-team",
    );
    map.insert("debian-fonts@lists.debian.org", "fonts-team");
    map.insert(
        "pkg-gnustep-maintainers@lists.alioth.debian.org",
        "gnustep-team",
    );
    map.insert(
        "pkg-gnome-maintainers@lists.alioth.debian.org",
        "gnome-team",
    );
    map.insert(
        "pkg-multimedia-maintainers@lists.alioth.debian.org",
        "multimedia-team",
    );
    map.insert("debian-ocaml-maint@lists.debian.org", "ocaml-team");
    map.insert("pkg-php-pear@lists.alioth.debian.org", "php-team/pear");
    map.insert("pkg-mpd-maintainers@lists.alioth.debian.org", "mpd-team");
    map.insert("pkg-cli-apps-team@lists.alioth.debian.org", "dotnet-team");
    map.insert("pkg-mono-group@lists.alioth.debian.org", "dotnet-team");
    map.insert("team+python@tracker.debian.org", "python-team/packages");
    map.insert("debian-med-packaging@lists.alioth.debian.org", "med-team");
    map.insert(
        "pkg-phototools-devel@lists.alioth.debian.org",
        "phototools-team",
    );
    map.insert("pkg-security-team@lists.alioth.debian.org", "security-team");
    map.insert(
        "pkg-systemd-maintainers@lists.alioth.debian.org",
        "systemd-team",
    );
    map.insert(
        "pkg-utopia-maintainers@lists.alioth.debian.org",
        "utopia-team",
    );
    map.insert("pkg-xfce-devel@lists.alioth.debian.org", "xfce-team");
    map.insert("pkg-kde-extras@lists.alioth.debian.org", "qt-kde-team");
    map.insert("debian-qt-kde@lists.debian.org", "qt-kde-team");
    map.insert("pkg-rust-maintainers@lists.alioth.debian.org", "rust-team");
    map.insert(
        "pkg-haskell-maintainers@lists.alioth.debian.org",
        "haskell-team",
    );
    map.insert(
        "pkg-electronics-devel@lists.alioth.debian.org",
        "electronics-team",
    );
    map.insert("pkg-lua-devel@lists.alioth.debian.org", "lua-team");
    map.insert("pkg-salt-team@lists.alioth.debian.org", "salt-team");
    map.insert("pkg-vim-maintainers@lists.alioth.debian.org", "vim-team");
    map.insert("debian-apache@lists.debian.org", "apache-team");
    map.insert("pkg-nagios-devel@lists.alioth.debian.org", "nagios-team");
    map.insert("pkg-samba-maint@lists.alioth.debian.org", "samba-team");
    map.insert("pkg-zope-developers@lists.alioth.debian.org", "zope-team");
    map.insert("pkg-db-devel@lists.alioth.debian.org", "db-team");
    map.insert(
        "pkg-openldap-devel@lists.alioth.debian.org",
        "openldap-team",
    );
    map.insert(
        "pkg-postgresql-public@lists.alioth.debian.org",
        "postgresql-team",
    );
    map.insert("pkg-mysql-maint@lists.alioth.debian.org", "mysql-team");
    map.insert("pkg-backup-devel@lists.alioth.debian.org", "backup-team");
    map.insert("pkg-groff-devel@lists.alioth.debian.org", "groff-team");
    map.insert("pkg-latex-devel@lists.alioth.debian.org", "latex-team");
    map.insert("pkg-r-pkg-team@lists.alioth.debian.org", "r-pkg-team");
    map.insert("pkg-scipy-devel@lists.alioth.debian.org", "scipy-team");
    map.insert(
        "pkg-hamradio-maintainers@lists.alioth.debian.org",
        "hamradio-team",
    );
    map.insert(
        "pkg-astro-maintainers@lists.alioth.debian.org",
        "astro-team",
    );
    map.insert("pkg-geography-devel@lists.alioth.debian.org", "gis-team");
    map.insert("pkg-grass-devel@lists.alioth.debian.org", "gis-team");
    map
}

/// Guess repository URL for a package hosted on Salsa
///
/// Args:
///   package: Package name
///   maintainer_email: The maintainer's email address (e.g. team list address)
/// Returns:
///   A guessed repository URL
pub fn guess_repository_url(package: &str, maintainer_email: &str) -> Option<String> {
    let team_name = if maintainer_email.ends_with("@debian.org") {
        maintainer_email.split('@').next().unwrap_or("").to_string()
    } else {
        let email_map = get_maintainer_email_map();
        email_map.get(maintainer_email)?.to_string()
    };

    if team_name.is_empty() {
        return None;
    }

    Some(format!(
        "https://salsa.debian.org/{}/{}.git",
        team_name, package
    ))
}

/// Determine the browser URL from a regular Git URL
pub fn determine_browser_url(vcs_type: &str, url: &Url) -> Option<String> {
    match vcs_type {
        "git" => {
            let url_str = url.as_str();
            // Handle common git hosting services
            if url_str.contains("github.com") {
                // Convert git URL to https browser URL
                if url_str.starts_with("ssh://") {
                    // SSH URL format: ssh://git@host/path/repo.git
                    let result = url_str.replace("ssh://git@", "https://");
                    Some(result.replace(".git", ""))
                } else {
                    Some(
                        url_str
                            .replace("git@github.com:", "https://github.com/")
                            .replace("git://github.com/", "https://github.com/")
                            .replace("git+ssh://git@github.com/", "https://github.com/")
                            .replace(".git", ""),
                    )
                }
            } else if url_str.contains("gitlab") || url_str.contains("salsa.debian.org") {
                if url_str.starts_with("https://") {
                    // Already an HTTPS URL, just remove .git
                    Some(url_str.replace(".git", ""))
                } else if url_str.starts_with("ssh://") {
                    // SSH URL format: ssh://git@host/path/repo.git
                    let result = url_str.replace("ssh://git@", "https://");
                    Some(result.replace(".git", ""))
                } else {
                    // SSH URL, convert to HTTPS
                    let mut result = url_str.to_string();

                    // Handle different SSH formats
                    if url_str.contains("@") {
                        // SSH format: git@host:user/repo.git
                        let parts: Vec<&str> = url_str.split('@').collect();
                        if parts.len() == 2 {
                            let host_and_path = parts[1];
                            let colon_parts: Vec<&str> = host_and_path.split(':').collect();
                            if colon_parts.len() == 2 {
                                result = format!("https://{}/{}", colon_parts[0], colon_parts[1]);
                            }
                        }
                    } else if url_str.starts_with("git://") {
                        // Git protocol: git://host/user/repo.git
                        result = url_str.replace("git://", "https://");
                    }

                    Some(result.replace(".git", ""))
                }
            } else {
                // For other git hosting services, try to convert to HTTPS
                if url_str.starts_with("git@") {
                    // SSH format: git@host:user/repo.git
                    let without_prefix = url_str.strip_prefix("git@").unwrap_or(url_str);
                    let parts: Vec<&str> = without_prefix.split(':').collect();
                    if parts.len() == 2 {
                        Some(format!(
                            "https://{}/{}",
                            parts[0],
                            parts[1].replace(".git", "")
                        ))
                    } else {
                        None
                    }
                } else if url_str.starts_with("git://") {
                    Some(url_str.replace("git://", "https://").replace(".git", ""))
                } else if url_str.starts_with("https://") {
                    Some(url_str.replace(".git", ""))
                } else {
                    None
                }
            }
        }
        _ => None,
    }
}

/// Get the VCS type and URL from a source control block
pub fn source_package_vcs(source: &Source) -> Result<(String, String), Error> {
    // Check for various VCS fields using the debian_control methods
    if let Some(url) = source.vcs_git() {
        return Ok(("Git".to_string(), url));
    }
    if let Some(url) = source.vcs_svn() {
        return Ok(("Svn".to_string(), url));
    }
    if let Some(url) = source.vcs_bzr() {
        return Ok(("Bzr".to_string(), url));
    }
    if let Some(url) = source.vcs_hg() {
        return Ok(("Hg".to_string(), url));
    }
    if let Some(url) = source.vcs_cvs() {
        return Ok(("Cvs".to_string(), url));
    }
    if let Some(url) = source.vcs_darcs() {
        return Ok(("Darcs".to_string(), url));
    }

    Err(Error::Other(
        "No VCS field found in source control".to_string(),
    ))
}

/// Update VCS fields in the control file
pub fn update_control_for_vcs_url(
    source: &mut Source,
    vcs_type: &str,
    repo_url: &str,
) -> Result<(), Error> {
    // Use the appropriate VCS setter method based on the type
    match vcs_type.to_lowercase().as_str() {
        "git" => source.set_vcs_git(repo_url),
        "svn" => source.set_vcs_svn(repo_url),
        "bzr" => source.set_vcs_bzr(repo_url),
        "hg" => source.set_vcs_hg(repo_url),
        "cvs" => source.set_vcs_cvs(repo_url),
        "darcs" => source.set_vcs_darcs(repo_url),
        _ => return Err(Error::Other(format!("Unsupported VCS type: {}", vcs_type))),
    }

    // Set browser URL if we can determine it
    if let Ok(url) = repo_url.parse::<Url>() {
        if let Some(browser_url) = determine_browser_url(&vcs_type.to_lowercase(), &url) {
            source.set_vcs_browser(Some(&browser_url));
        }
    }

    Ok(())
}

/// Update official VCS information in debian/control
pub fn update_official_vcs(
    wt: &dyn PyWorkingTree,
    subpath: &Path,
    repo_url: Option<&str>,
    committer: Option<&str>,
    force: bool,
    _create: bool,
) -> Result<String, Error> {
    // Check if tree is clean
    if !force && wt.has_changes()? {
        return Err(Error::UncommittedChanges);
    }

    let debian_path = subpath.join("debian");
    let control_path = debian_path.join("control");

    // For now, we only handle regular debian/control files
    if !wt.has_filename(&control_path) {
        return Err(Error::Other("No debian/control file found".to_string()));
    }

    // Read and parse the control file
    let control_content = wt.get_file_text(&control_path)?;
    let control_str = String::from_utf8(control_content)
        .map_err(|e| Error::Other(format!("Failed to parse control file as UTF-8: {}", e)))?;

    let control = control_str
        .parse::<Control>()
        .map_err(|e| Error::Other(format!("Failed to parse control file: {}", e)))?;

    let source = control
        .source()
        .ok_or_else(|| Error::Other("No source package found".to_string()))?;

    // Check if VCS is already specified
    if let Ok((vcs_type, url)) = source_package_vcs(&source) {
        if !force {
            return Err(Error::Other(format!(
                "VCS already specified: {} {}",
                vcs_type, url
            )));
        }
    }

    // Get maintainer email for guessing repository URL
    let maintainer = source
        .maintainer()
        .ok_or_else(|| Error::Other("No Maintainer field found".to_string()))?;

    // Extract email from maintainer field (format: "Name <email>")
    let maintainer_email = if let Some(start) = maintainer.find('<') {
        if let Some(end) = maintainer.find('>') {
            &maintainer[start + 1..end]
        } else {
            &maintainer
        }
    } else {
        &maintainer
    };

    // Get source package name
    let source_name = source
        .name()
        .ok_or_else(|| Error::Other("No Source field found".to_string()))?;

    // Determine repository URL
    let repo_url = if let Some(url) = repo_url {
        url.to_string()
    } else {
        guess_repository_url(&source_name, maintainer_email)
            .ok_or_else(|| Error::Other("Unable to guess repository URL".to_string()))?
    };

    log::info!("Using repository URL: {}", repo_url);

    // Determine VCS type - for now, assume Git
    let vcs_type = "Git";

    // Since we can't mutably borrow the source from control, we need to recreate it
    // Parse the control file as a string and modify it directly
    let mut modified_content = control_str.clone();

    // This is a simple approach - we'll insert the VCS fields after the source paragraph
    // Find the end of the source paragraph (indicated by a blank line or end of file)
    let source_end = if let Some(pos) = modified_content.find("\n\n") {
        pos
    } else {
        modified_content.len()
    };

    // Insert the VCS fields
    let vcs_fields = format!("Vcs-Git: {}\n", repo_url);
    let browser_fields = if let Ok(url) = repo_url.parse::<Url>() {
        if let Some(browser_url) = determine_browser_url(&vcs_type.to_lowercase(), &url) {
            format!("Vcs-Browser: {}\n", browser_url)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    modified_content.insert_str(source_end, &format!("{}{}", vcs_fields, browser_fields));

    // Write the updated control file
    wt.put_file_bytes_non_atomic(&control_path, modified_content.as_bytes())?;

    // Commit the changes
    if let Some(committer) = committer {
        match wt.commit(
            "Set Vcs headers.",
            Some(committer),
            None,        // timestamp
            Some(false), // allow_pointless
            None,        // specific_files
        ) {
            Ok(_) => {}
            Err(BrzError::PointlessCommit) => {
                if !force {
                    return Err(Error::Other("No changes to commit".to_string()));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(repo_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::tree::{MutableTree, Tree};
    use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};

    fn init_working_tree(path: &Path) -> GenericWorkingTree {
        use breezyshim::controldir::ControlDirFormat;
        breezyshim::init();

        let format = ControlDirFormat::default();
        let transport =
            breezyshim::transport::get_transport(&url::Url::from_file_path(path).unwrap(), None)
                .unwrap();

        let controldir = format.initialize_on_transport(&transport).unwrap();
        controldir.create_repository(None).unwrap();
        controldir.create_branch(None).unwrap();
        controldir.create_workingtree().unwrap()
    }

    /// Create a working tree with a committed debian/control file.
    fn tree_with_control(control: &str) -> (tempfile::TempDir, GenericWorkingTree) {
        let td = tempfile::tempdir().unwrap();
        let wt = init_working_tree(td.path());
        wt.mkdir(Path::new("debian")).unwrap();
        wt.put_file_bytes_non_atomic(Path::new("debian/control"), control.as_bytes())
            .unwrap();
        wt.add(&[Path::new("debian"), Path::new("debian/control")])
            .unwrap();
        wt.build_commit()
            .message("Initial")
            .committer("Test <test@example.com>")
            .commit()
            .unwrap();
        (td, wt)
    }

    const CONTROL: &str = "Source: test-package\nMaintainer: Test User <test@example.com>\n\nPackage: test-package\nArchitecture: all\n";

    #[test]
    fn test_update_official_vcs_explicit_url() {
        let (_td, wt) = tree_with_control(CONTROL);
        let result = update_official_vcs(
            &wt,
            Path::new(""),
            Some("https://github.com/user/test-package.git"),
            Some("Test <test@example.com>"),
            false,
            false,
        )
        .unwrap();
        assert_eq!(result, "https://github.com/user/test-package.git");

        let content =
            String::from_utf8(wt.get_file_text(Path::new("debian/control")).unwrap()).unwrap();
        assert!(content.contains("Vcs-Git: https://github.com/user/test-package.git"));
        assert!(content.contains("Vcs-Browser: https://github.com/user/test-package"));
    }

    #[test]
    fn test_update_official_vcs_uncommitted_changes() {
        let (_td, wt) = tree_with_control(CONTROL);
        // Introduce an uncommitted change.
        wt.put_file_bytes_non_atomic(Path::new("debian/control"), b"Source: changed\n")
            .unwrap();
        let result = update_official_vcs(
            &wt,
            Path::new(""),
            Some("https://github.com/user/test-package.git"),
            Some("Test <test@example.com>"),
            false,
            false,
        );
        assert!(matches!(result, Err(Error::UncommittedChanges)));
    }

    #[test]
    fn test_update_official_vcs_already_specified() {
        let control = "Source: test-package\nMaintainer: Test User <test@example.com>\nVcs-Git: https://example.com/existing.git\n\nPackage: test-package\nArchitecture: all\n";
        let (_td, wt) = tree_with_control(control);
        let result = update_official_vcs(
            &wt,
            Path::new(""),
            Some("https://github.com/user/test-package.git"),
            Some("Test <test@example.com>"),
            false,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_update_official_vcs_guessed_from_maintainer() {
        // No explicit URL: the salsa URL is guessed from the team maintainer.
        let control = "Source: test-package\nMaintainer: Debian Rust Team <pkg-rust-maintainers@lists.alioth.debian.org>\n\nPackage: test-package\nArchitecture: all\n";
        let (_td, wt) = tree_with_control(control);
        let result = update_official_vcs(
            &wt,
            Path::new(""),
            None,
            Some("Test <test@example.com>"),
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            result,
            "https://salsa.debian.org/rust-team/test-package.git"
        );
    }

    #[test]
    fn test_guess_repository_url_debian_org() {
        let result = guess_repository_url("test-package", "someone@debian.org");
        assert_eq!(
            result,
            Some("https://salsa.debian.org/someone/test-package.git".to_string())
        );
    }

    #[test]
    fn test_guess_repository_url_team_list() {
        let result = guess_repository_url(
            "test-package",
            "pkg-javascript-devel@lists.alioth.debian.org",
        );
        assert_eq!(
            result,
            Some("https://salsa.debian.org/js-team/test-package.git".to_string())
        );
    }

    #[test]
    fn test_guess_repository_url_unknown_email() {
        let result = guess_repository_url("test-package", "unknown@example.com");
        assert_eq!(result, None);
    }

    #[test]
    fn test_determine_browser_url_github_ssh() {
        let url = Url::parse("ssh://git@github.com/user/repo.git").unwrap();
        let result = determine_browser_url("git", &url);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_determine_browser_url_github_https() {
        let url = Url::parse("https://github.com/user/repo.git").unwrap();
        let result = determine_browser_url("git", &url);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_determine_browser_url_salsa_ssh() {
        let url = Url::parse("ssh://git@salsa.debian.org/team/repo.git").unwrap();
        let result = determine_browser_url("git", &url);
        assert_eq!(
            result,
            Some("https://salsa.debian.org/team/repo".to_string())
        );
    }

    #[test]
    fn test_determine_browser_url_salsa_https() {
        let url = Url::parse("https://salsa.debian.org/team/repo.git").unwrap();
        let result = determine_browser_url("git", &url);
        assert_eq!(
            result,
            Some("https://salsa.debian.org/team/repo".to_string())
        );
    }

    #[test]
    fn test_no_vcs_location_display() {
        assert_eq!(
            NoVcsLocation.to_string(),
            "No VCS location specified or determined"
        );
    }

    #[test]
    fn test_vcs_already_specified_display() {
        let err = VcsAlreadySpecified {
            vcs_type: "Git".to_string(),
            url: "https://example.com/repo.git".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Vcs is already specified: Git https://example.com/repo.git"
        );
    }

    #[test]
    fn test_determine_browser_url_non_git_vcs() {
        let url = Url::parse("https://example.com/repo.git").unwrap();
        assert_eq!(determine_browser_url("svn", &url), None);
    }

    #[test]
    fn test_determine_browser_url_gitlab_git_ssh() {
        // gitlab host, "git+ssh://git@host/user/repo.git" form: neither plain
        // https:// nor ssh://, but contains "@", so it reaches the `@`-split
        // fallback. The host-and-path part has no colon, so the rewrite does
        // not fire and only the ".git" suffix is stripped.
        let url = Url::parse("git+ssh://git@gitlab.example.com/user/repo.git").unwrap();
        assert_eq!(
            determine_browser_url("git", &url),
            Some("git+ssh://git@gitlab.example.com/user/repo".to_string())
        );
    }

    #[test]
    fn test_determine_browser_url_gitlab_git_ssh_with_port() {
        // A port in the authority gives the host-and-path part a colon, so the
        // `colon_parts.len() == 2` rewrite fires, turning it into an https URL.
        let url = Url::parse("git+ssh://git@gitlab.example.com:22/user/repo.git").unwrap();
        assert_eq!(
            determine_browser_url("git", &url),
            Some("https://gitlab.example.com/22/user/repo".to_string())
        );
    }

    #[test]
    fn test_determine_browser_url_other_host_git_protocol() {
        let url = Url::parse("git://git.example.com/user/repo.git").unwrap();
        assert_eq!(
            determine_browser_url("git", &url),
            Some("https://git.example.com/user/repo".to_string())
        );
    }

    #[test]
    fn test_determine_browser_url_other_host_https() {
        let url = Url::parse("https://git.example.com/user/repo.git").unwrap();
        assert_eq!(
            determine_browser_url("git", &url),
            Some("https://git.example.com/user/repo".to_string())
        );
    }

    #[test]
    fn test_source_package_vcs_git() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        source.set_vcs_git("https://example.com/repo.git");
        assert_eq!(
            source_package_vcs(&source).unwrap(),
            (
                "Git".to_string(),
                "https://example.com/repo.git".to_string()
            )
        );
    }

    #[test]
    fn test_source_package_vcs_svn() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        source.set_vcs_svn("svn://example.com/repo");
        assert_eq!(
            source_package_vcs(&source).unwrap(),
            ("Svn".to_string(), "svn://example.com/repo".to_string())
        );
    }

    #[test]
    fn test_source_package_vcs_bzr() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        source.set_vcs_bzr("bzr://example.com/repo");
        assert_eq!(
            source_package_vcs(&source).unwrap(),
            ("Bzr".to_string(), "bzr://example.com/repo".to_string())
        );
    }

    #[test]
    fn test_source_package_vcs_none() {
        let mut control = Control::new();
        let source = control.add_source("test");
        assert!(source_package_vcs(&source).is_err());
    }

    #[test]
    fn test_update_control_for_vcs_url_git() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "git", "https://github.com/user/repo.git").unwrap();
        assert_eq!(
            source.vcs_git(),
            Some("https://github.com/user/repo.git".to_string())
        );
        // The browser URL is derived for a recognised host.
        assert_eq!(
            source.vcs_browser(),
            Some("https://github.com/user/repo".to_string())
        );
    }

    #[test]
    fn test_update_control_for_vcs_url_svn() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "svn", "svn://example.com/repo").unwrap();
        assert_eq!(source.vcs_svn(), Some("svn://example.com/repo".to_string()));
    }

    #[test]
    fn test_update_control_for_vcs_url_bzr() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "bzr", "bzr://example.com/repo").unwrap();
        assert_eq!(source.vcs_bzr(), Some("bzr://example.com/repo".to_string()));
    }

    #[test]
    fn test_update_control_for_vcs_url_hg() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "hg", "https://example.com/repo").unwrap();
        assert_eq!(
            source.vcs_hg(),
            Some("https://example.com/repo".to_string())
        );
    }

    #[test]
    fn test_update_control_for_vcs_url_cvs() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "cvs", ":pserver:example.com:/cvsroot").unwrap();
        assert_eq!(
            source.vcs_cvs(),
            Some(":pserver:example.com:/cvsroot".to_string())
        );
    }

    #[test]
    fn test_update_control_for_vcs_url_darcs() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        update_control_for_vcs_url(&mut source, "darcs", "https://example.com/repo").unwrap();
        assert_eq!(
            source.vcs_darcs(),
            Some("https://example.com/repo".to_string())
        );
    }

    #[test]
    fn test_update_control_for_vcs_url_unsupported() {
        let mut control = Control::new();
        let mut source = control.add_source("test");
        assert!(
            update_control_for_vcs_url(&mut source, "fossil", "https://example.com/repo").is_err()
        );
    }
}
