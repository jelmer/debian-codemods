use crate::{FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::abstract_control::AbstractSource;
use debian_analyzer::control::TemplatedControlEditor;
use debian_changelog::parseaddr;
use std::collections::HashMap;
use std::path::Path;
use url::Url;

const OBSOLETE_HOSTS: &[&str] = &[
    "anonscm.debian.org",
    "alioth.debian.org",
    "svn.debian.org",
    "git.debian.org",
    "bzr.debian.org",
    "hg.debian.org",
];

/// Check if a URL is on an obsolete Debian infrastructure host
fn is_on_obsolete_host(url: &str) -> bool {
    if let Ok(parsed_url) = Url::parse(url) {
        if let Some(host) = parsed_url.host_str() {
            // Strip user part if present (e.g., git@host -> host)
            let host_without_user = host.split('@').next_back().unwrap_or(host);
            return OBSOLETE_HOSTS.contains(&host_without_user);
        }
    }
    false
}

/// Query vcswatch database for package VCS information
#[cfg(feature = "udd")]
async fn retrieve_vcswatch_urls(
    package: &str,
) -> Result<Option<(String, String, Option<String>)>, FixerError> {
    use sqlx::Row;

    let client = debian_analyzer::udd::connect_udd_mirror()
        .await
        .map_err(|e| FixerError::Other(format!("Failed to connect to UDD: {}", e)))?;

    let query = "SELECT vcs, url, browser FROM vcswatch WHERE source = $1";

    let row = sqlx::query(query)
        .bind(package)
        .fetch_optional(&client)
        .await
        .map_err(|e| FixerError::Other(format!("Failed to query vcswatch: {}", e)))?;

    if let Some(row) = row {
        let vcs_type: String = row.get(0);
        let url: String = row.get(1);
        let browser: Option<String> = row.get(2);
        Ok(Some((vcs_type, url, browser)))
    } else {
        Ok(None)
    }
}

#[cfg(not(feature = "udd"))]
async fn retrieve_vcswatch_urls(
    _package: &str,
) -> Result<Option<(String, String, Option<String>)>, FixerError> {
    Ok(None)
}

/// Determine browser URL from VCS URL
fn determine_browser_url(vcs_type: &str, vcs_url: &str) -> Option<String> {
    debian_analyzer::vcs::determine_browser_url(vcs_type, vcs_url, None).map(|u| u.to_string())
}

/// Determine Salsa browser URL from Git URL
fn determine_salsa_browser_url(url: &str) -> Option<String> {
    if let Ok(parsed_url) = Url::parse(url) {
        if let Some(host) = parsed_url.host_str() {
            if host == "salsa.debian.org" {
                // For salsa.debian.org, just strip .git extension from browser URL
                let path = parsed_url.path().trim_end_matches(".git");
                return Some(format!("https://salsa.debian.org{}", path));
            }
        }
    }
    None
}

/// Verify that a Salsa repository exists by checking the browser URL
async fn verify_salsa_repository(url: &str) -> Result<bool, FixerError> {
    let browser_url = determine_salsa_browser_url(url)
        .ok_or_else(|| FixerError::Other("Not a salsa URL".to_string()))?;

    let client = reqwest::Client::builder()
        .user_agent("lintian-brush")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| FixerError::Other(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(&browser_url)
        .send()
        .await
        .map_err(|e| FixerError::Other(format!("Failed to fetch URL: {}", e)))?;

    Ok(response.status().is_success())
}

/// Mapping of old team names to new ones
fn get_team_name_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("debian-xml-sgml", "xml-sgml-team");
    map.insert("pkg-go", "go-team");
    map.insert("pkg-fonts", "fonts-team");
    map.insert("pkg-javascript", "js-team");
    map.insert("pkg-java", "java-team");
    map.insert("pkg-mpd", "mpd-team");
    map.insert("pkg-electronics", "electronics-team");
    map.insert("pkg-xfce", "xfce-team");
    map.insert("pkg-lxc", "lxc-team");
    map.insert("debian-science", "science-team");
    map.insert("pkg-games", "games-team");
    map.insert("pkg-bluetooth", "bluetooth-team");
    map.insert("debichem", "debichem-team");
    map.insert("openstack", "openstack-team");
    map.insert("pkg-kde", "qt-kde-team");
    map.insert("debian-islamic", "islamic-team");
    map.insert("pkg-lua", "lua-team");
    map.insert("pkg-xorg", "xorg-team");
    map.insert("debian-astro", "debian-astro-team");
    map.insert("pkg-icecast", "multimedia-team");
    map.insert("glibc-bsd", "bsd-team");
    map.insert("pkg-nvidia", "nvidia-team");
    map.insert("pkg-llvm", "llvm-team");
    map.insert("pkg-nagios", "nagios-team");
    map.insert("pkg-sugar", "pkg-sugar-team");
    map.insert("pkg-phototools", "debian-phototools-team");
    map.insert("pkg-netmeasure", "ineteng-team");
    map.insert("pkg-hamradio", "debian-hamradio-team");
    map.insert("pkg-sass", "sass-team");
    map.insert("pkg-rpm", "pkg-rpm-team");
    map.insert("tts", "tts-team");
    map.insert("python-apps", "python-team/applications");
    map.insert("pkg-monitoring", "monitoring-team");
    map.insert("pkg-perl", "perl-team/modules");
    map.insert("debian-iot", "debian-iot-team");
    map.insert("pkg-bitcoin", "cryptocoin-team");
    map.insert("pkg-cyrus-imapd", "debian");
    map.insert("pkg-dns", "dns-team");
    map.insert("pkg-freeipa", "freeipa-team");
    map.insert("pkg-ocaml-team", "ocaml-team");
    map.insert("pkg-vdr-dvb", "vdr-team");
    map.insert("debian-in", "debian-in-team");
    map.insert("pkg-octave", "pkg-octave-team");
    map.insert("pkg-postgresql", "postgresql");
    map.insert("pkg-grass", "debian-gis-team");
    map.insert("pkg-evolution", "gnome-team");
    map.insert("pkg-gnome", "gnome-team");
    map.insert("pkg-exppsy", "neurodebian-team");
    map.insert("pkg-voip", "pkg-voip-team");
    map.insert("pkg-privacy", "pkg-privacy-team");
    map.insert("pkg-libvirt", "libvirt-team");
    map.insert("debian-ha", "ha-team");
    map.insert("debian-lego", "debian-lego-team");
    map.insert("calendarserver", "calendarserver-team");
    map.insert("3dprinter", "3dprinting-team");
    map.insert("pkg-multimedia", "multimedia-team");
    map.insert("pkg-emacsen", "emacsen-team");
    map.insert("pkg-haskell", "haskell-team");
    map.insert("pkg-gnutls", "gnutls-team");
    map.insert("pkg-mysql", "mariadb-team");
    map.insert("pkg-php", "php-team");
    map.insert("pkg-qemu", "qemu-team");
    map.insert("pkg-xmpp", "xmpp-team");
    map.insert("uefi", "efi-team");
    map.insert("pkg-manpages-fr", "l10n-fr-team");
    map.insert("pkg-proftpd", "debian-proftpd-team");
    map.insert("pkg-apache", "apache-team");
    map
}

/// Mapping of Git path renames from old infrastructure to Salsa
fn get_git_path_renames() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("pkg-kde/applications", "qt-kde-team/kde");
    map.insert("3dprinter/packages", "3dprinting-team");
    map.insert("pkg-emacsen/pkg", "emacsen-team");
    map.insert("debian-astro/packages", "debian-astro-team");
    map.insert("debian-islamic/packages", "islamic-team");
    map.insert("debichem/packages", "debichem-team");
    map.insert("pkg-privacy/packages", "pkg-privacy-team");
    map.insert("pkg-cli-libs/packages", "dotnet-team");
    map
}

/// Guess the Salsa path from an alioth URL
fn salsa_path_from_alioth_url(vcs_type: &str, alioth_url: &str) -> Option<String> {
    let team_name_map = get_team_name_map();
    let git_path_renames = get_git_path_renames();

    if vcs_type.to_lowercase() == "git" {
        // Handle collab-maint repositories
        let pat = regex::Regex::new(
            r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?collab-maint/",
        )
        .ok()?;
        if pat.is_match(alioth_url) {
            return Some(pat.replace(alioth_url, "debian/").to_string());
        }

        // Handle users repositories
        let pat =
            regex::Regex::new(r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?users/")
                .ok()?;
        if pat.is_match(alioth_url) {
            return Some(pat.replace(alioth_url, "").to_string());
        }

        // General pattern matching for team repositories
        if let Some(caps) =
            regex::Regex::new(r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?(.+)")
                .ok()?
                .captures(alioth_url)
        {
            let path = caps.get(4)?.as_str();
            let parts: Vec<&str> = path.split('/').collect();

            // Check git path renames
            for i in (1..=parts.len()).rev() {
                let subpath = parts[..i].join("/");
                if let Some(new_path) = git_path_renames.get(subpath.as_str()) {
                    let remaining = parts[i..].join("/");
                    return if remaining.is_empty() {
                        Some(new_path.to_string())
                    } else {
                        Some(format!("{}/{}", new_path, remaining))
                    };
                }
            }

            // Handle special case for debian-in fonts
            if let Some(first_part) = parts.first() {
                if *first_part == "debian-in" && alioth_url.contains("fonts-") {
                    return Some(format!("fonts-team/{}", parts[1..].join("/")));
                }
                // Check team name map
                if let Some(new_name) = team_name_map.get(first_part) {
                    return Some(format!("{}/{}", new_name, parts[1..].join("/")));
                }
            }
        }

        // Handle alioth.debian.org/anonscm URLs
        if let Some(caps) =
            regex::Regex::new(r"https?://alioth\.debian\.org/anonscm/(git/|cgit/)?([^/]+)/")
                .ok()?
                .captures(alioth_url)
        {
            let team = caps.get(2)?.as_str();
            if let Some(new_name) = team_name_map.get(team) {
                return Some(alioth_url.replace(&format!("{}/", team), &format!("{}/", new_name)));
            }
        }
    } else if vcs_type.to_lowercase() == "svn" {
        // Handle SVN pkg-perl repositories
        if alioth_url.starts_with("svn://svn.debian.org/pkg-perl/trunk") {
            return Some(alioth_url.replace(
                "svn://svn.debian.org/pkg-perl/trunk",
                "perl-team/modules/packages",
            ));
        }
        // Handle SVN pkg-lua repositories
        if alioth_url.starts_with("svn://svn.debian.org/pkg-lua/packages") {
            return Some(alioth_url.replace("svn://svn.debian.org/pkg-lua/packages", "lua-team"));
        }

        // Parse SVN URLs and apply team name transformations
        if let Ok(parsed_url) = Url::parse(alioth_url) {
            if parsed_url.scheme() == "svn"
                && (parsed_url.host_str() == Some("svn.debian.org")
                    || parsed_url.host_str() == Some("anonscm.debian.org"))
            {
                let mut parts: Vec<&str> = parsed_url
                    .path()
                    .trim_start_matches('/')
                    .split('/')
                    .collect();
                if parts.first() == Some(&"svn") {
                    parts.remove(0);
                }

                // Handle various SVN repository patterns
                if parts.len() == 3 && team_name_map.contains_key(parts[0]) && parts[2] == "trunk" {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[1]));
                }
                if parts.len() == 3 && team_name_map.contains_key(parts[0]) && parts[1] == "trunk" {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[2]));
                }
                if parts.len() == 4
                    && team_name_map.contains_key(parts[0])
                    && parts[1] == "packages"
                    && parts[3] == "trunk"
                {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[2]));
                }
                if parts.len() == 4
                    && team_name_map.contains_key(parts[0])
                    && parts[1] == "trunk"
                    && parts[2] == "packages"
                {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[3]));
                }
                if parts.len() > 3
                    && team_name_map.contains_key(parts[0])
                    && parts[parts.len() - 2] == "trunk"
                {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[parts.len() - 1]));
                }
                if parts.len() == 3
                    && team_name_map.contains_key(parts[0])
                    && (parts[1] == "packages" || parts[1] == "unstable")
                {
                    let team = team_name_map[parts[0]];
                    return Some(format!("{}/{}", team, parts[2]));
                }
            }
        }
    }

    None
}

/// Convert an alioth URL to a Salsa URL
fn salsa_url_from_alioth_url(vcs_type: &str, alioth_url: &str) -> Option<String> {
    let mut path = salsa_path_from_alioth_url(vcs_type, alioth_url)?;
    path = path.trim_end_matches('/').to_string();
    if !path.ends_with(".git") {
        path.push_str(".git");
    }
    Some(format!("https://salsa.debian.org/{}", path))
}

/// Error for when a new repository URL cannot be determined
struct NewRepositoryURLUnknown;

/// Find new VCS URLs for a package migrating from obsolete infrastructure
async fn find_new_urls(
    vcs_type: &str,
    vcs_url: &str,
    package: &str,
    maintainer_email: &str,
    preferences: &FixerPreferences,
) -> Result<(String, String, Option<String>), NewRepositoryURLUnknown> {
    let net_access = preferences.net_access.unwrap_or(false);

    // First, try following redirects for HTTP/HTTPS URLs
    if net_access && (vcs_url.starts_with("https://") || vcs_url.starts_with("http://")) {
        if let Ok(client) = reqwest::Client::builder()
            .user_agent("lintian-brush")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
        {
            if let Ok(response) = client.get(vcs_url).send().await {
                let redirected_url = response.url().to_string();
                if !is_on_obsolete_host(&redirected_url) {
                    let vcs_browser = determine_browser_url(vcs_type, &redirected_url);
                    return Ok((vcs_type.to_string(), redirected_url, vcs_browser));
                }
            }
        }
    }

    // Try vcswatch database if network access is allowed
    if net_access {
        if let Ok(Some((db_vcs_type, db_vcs_url, db_vcs_browser))) =
            retrieve_vcswatch_urls(package).await
        {
            if !is_on_obsolete_host(&db_vcs_url) {
                let vcs_browser = if let Some(browser) = db_vcs_browser {
                    if is_on_obsolete_host(&browser) {
                        determine_browser_url(&db_vcs_type, &db_vcs_url).or(Some(browser))
                    } else {
                        Some(browser)
                    }
                } else {
                    determine_browser_url(&db_vcs_type, &db_vcs_url)
                };
                return Ok((db_vcs_type, db_vcs_url, vcs_browser));
            }
        }
    }

    // Try to guess repository URL from maintainer email
    let guessed_url = debian_analyzer::salsa::guess_repository_url(package, maintainer_email);
    let (new_vcs_type, new_vcs_url) = if let Some(url) = guessed_url {
        ("Git".to_string(), url.to_string())
    } else {
        // Fall back to converting alioth URL to Salsa
        let converted_url =
            salsa_url_from_alioth_url(vcs_type, vcs_url).ok_or(NewRepositoryURLUnknown)?;
        ("Git".to_string(), converted_url)
    };

    // Verify repository exists if network access is allowed
    if net_access && !verify_salsa_repository(&new_vcs_url).await.unwrap_or(false) {
        return Err(NewRepositoryURLUnknown);
    }

    let vcs_browser = determine_salsa_browser_url(&new_vcs_url);
    Ok((new_vcs_type, new_vcs_url, vcs_browser))
}

/// Main function to migrate VCS fields from obsolete infrastructure
pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let mut source = editor.source().ok_or(FixerError::NoChanges)?;

    // Get VCS info
    let (vcs_type, vcs_url) = if let Some(url) = source.get_vcs_url("Git") {
        ("Git", url)
    } else if let Some(url) = source.get_vcs_url("Svn") {
        ("Svn", url)
    } else if let Some(url) = source.get_vcs_url("Bzr") {
        ("Bzr", url)
    } else if let Some(url) = source.get_vcs_url("Hg") {
        ("Hg", url)
    } else if let Some(url) = source.get_vcs_url("Cvs") {
        ("Cvs", url)
    } else {
        return Err(FixerError::NoChanges);
    };

    // Check if on obsolete host
    if !is_on_obsolete_host(&vcs_url) {
        return Err(FixerError::NoChanges);
    }

    // Get package and maintainer info
    let paragraph = source.as_mut_deb822();
    let package = paragraph
        .get("Source")
        .ok_or(FixerError::NoChanges)?
        .to_string();
    let maintainer = paragraph
        .get("Maintainer")
        .ok_or(FixerError::NoChanges)?
        .to_string();
    let (_, maintainer_email) = parseaddr(&maintainer);

    let old_vcs_browser = source.get_vcs_url("Browser");
    let old_vcs_type = vcs_type.to_string();
    let old_vcs_url = vcs_url.clone();

    // Find new URLs
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FixerError::Other(format!("Failed to create runtime: {}", e)))?;

    let (new_vcs_type, new_vcs_url, new_vcs_browser) = rt
        .block_on(find_new_urls(
            vcs_type,
            &vcs_url,
            &package,
            maintainer_email,
            preferences,
        ))
        .map_err(|_| FixerError::NoChanges)?;

    // Record fixed lintian issues
    let mut fixed_issues = Vec::new();

    let issue = LintianIssue::source_with_info(
        "vcs-obsolete-in-debian-infrastructure",
        vec![format!(
            "vcs-{} {}",
            old_vcs_type.to_lowercase(),
            old_vcs_url
        )],
    );

    if !issue.should_fix(base_path) {
        return Err(FixerError::NoChangesAfterOverrides(vec![issue]));
    }

    fixed_issues.push(issue);

    // Check for vcs-field-bitrotted tag (CVS and ViewVC cases)
    let vcs_cvs = source.get_vcs_url("Cvs");
    let is_cvs_bitrotted = if let Some(cvs_url) = vcs_cvs {
        regex::Regex::new(r"@(?:cvs\.alioth|anonscm)\.debian\.org:/cvsroot/")
            .ok()
            .map(|re| re.is_match(&cvs_url))
            .unwrap_or(false)
    } else {
        false
    };

    let is_svn_viewvc_bitrotted = vcs_type == "Svn"
        && old_vcs_browser
            .as_ref()
            .map(|b| b.contains("viewvc"))
            .unwrap_or(false);

    if is_cvs_bitrotted || is_svn_viewvc_bitrotted {
        let info = format!(
            "{} {}",
            old_vcs_url,
            old_vcs_browser.as_deref().unwrap_or("")
        );
        let issue = LintianIssue::source_with_info("vcs-field-bitrotted", vec![info]);
        if issue.should_fix(base_path) {
            fixed_issues.push(issue);
        }
    }

    // Remove old VCS fields
    {
        let paragraph = source.as_mut_deb822();
        for hdr in ["Vcs-Git", "Vcs-Bzr", "Vcs-Hg", "Vcs-Svn", "Vcs-Cvs"] {
            if hdr != format!("Vcs-{}", new_vcs_type) {
                paragraph.remove(hdr);
            }
        }
        if new_vcs_browser.is_none() {
            paragraph.remove("Vcs-Browser");
        }
    }

    // Set new VCS fields
    source.set_vcs_url(&new_vcs_type, &new_vcs_url);
    if let Some(browser) = new_vcs_browser {
        source.set_vcs_url("Browser", &browser);
    }

    editor.commit()?;

    let description = "Update Vcs-* headers to use salsa repository.".to_string();

    Ok(FixerResult::builder(&description)
        .fixed_issues(fixed_issues)
        .build())
}

declare_fixer! {
    name: "vcs-field-bitrotted",
    tags: ["vcs-obsolete-in-debian-infrastructure", "vcs-field-bitrotted"],
    // Must fix infrastructure changes before checking for broken URIs
    before: ["vcs-broken-uri"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_on_obsolete_host() {
        assert!(is_on_obsolete_host(
            "git://git.debian.org/jelmer/lintian-brush"
        ));
        assert!(is_on_obsolete_host(
            "https://anonscm.debian.org/git/jelmer/lintian-brush"
        ));
        assert!(!is_on_obsolete_host(
            "https://salsa.debian.org/jelmer/lintian-brush.git"
        ));
    }

    #[test]
    fn test_salsa_url_from_alioth_url_team() {
        // Test conversion for a team repository
        let result =
            salsa_url_from_alioth_url("Git", "git://git.debian.org/pkg-javascript/node-foo");
        assert_eq!(
            result,
            Some("https://salsa.debian.org/js-team/node-foo.git".to_string())
        );
    }

    #[test]
    fn test_salsa_url_from_alioth_url_personal() {
        // Personal repositories (not matching any team pattern) return None
        // These are handled via guess_repository_url in find_new_urls
        let result = salsa_url_from_alioth_url("Git", "git://git.debian.org/jelmer/lintian-brush");
        assert_eq!(result, None);
    }

    #[test]
    fn test_simple_migration() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\n\
             Maintainer: Jelmer Vernooij <jelmer@debian.org>\n\
             Vcs-Git: git://git.debian.org/jelmer/lintian-brush\n\
             Vcs-Browser: https://alioth.debian.org/git/jelmer/lintian-brush\n\n\
             Package: lintian-brush\n\
             Description: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences).unwrap();
        assert!(result
            .description
            .contains("Update Vcs-* headers to use salsa repository"));

        let content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(content.contains("Vcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git"));
        assert!(content.contains("Vcs-Browser: https://salsa.debian.org/jelmer/lintian-brush"));
    }

    #[test]
    fn test_already_on_salsa() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: lintian-brush\n\
             Maintainer: Jelmer Vernooij <jelmer@debian.org>\n\
             Vcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git\n\
             Vcs-Browser: https://salsa.debian.org/jelmer/lintian-brush\n\n\
             Package: lintian-brush\n\
             Description: Testing\n Test test\n",
        )
        .unwrap();

        let preferences = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
