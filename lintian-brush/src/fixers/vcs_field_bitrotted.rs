use crate::declare_detector;
use crate::diagnostic::{Action, Deb822Action, Diagnostic, ParagraphSelector};
use debian_workspace::Workspace;
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_changelog::parseaddr;
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;

const OBSOLETE_HOSTS: &[&str] = &[
    "anonscm.debian.org",
    "alioth.debian.org",
    "svn.debian.org",
    "git.debian.org",
    "bzr.debian.org",
    "hg.debian.org",
];

fn is_on_obsolete_host(url: &str) -> bool {
    if let Ok(parsed) = Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            let host_without_user = host.split('@').next_back().unwrap_or(host);
            return OBSOLETE_HOSTS.contains(&host_without_user);
        }
    }
    false
}

#[cfg(feature = "udd")]
async fn retrieve_vcswatch_urls(
    package: &str,
) -> Result<Option<(String, String, Option<String>)>, FixerError> {
    use sqlx::Row;
    let client = debian_analyzer::udd::connect_udd_mirror()
        .await
        .map_err(|e| FixerError::Other(format!("Failed to connect to UDD: {}", e)))?;
    let row = sqlx::query("SELECT vcs, url, browser FROM vcswatch WHERE source = $1")
        .bind(package)
        .fetch_optional(&client)
        .await
        .map_err(|e| FixerError::Other(format!("Failed to query vcswatch: {}", e)))?;
    Ok(row.map(|r| {
        (
            r.get::<String, _>(0),
            r.get::<String, _>(1),
            r.get::<Option<String>, _>(2),
        )
    }))
}

#[cfg(not(feature = "udd"))]
async fn retrieve_vcswatch_urls(
    _package: &str,
) -> Result<Option<(String, String, Option<String>)>, FixerError> {
    Ok(None)
}

fn determine_browser_url(vcs_type: &str, vcs_url: &str) -> Option<String> {
    debian_analyzer::vcs::determine_browser_url(vcs_type, vcs_url, None).map(|u| u.to_string())
}

fn determine_salsa_browser_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.host_str()? != "salsa.debian.org" {
        return None;
    }
    let path = parsed.path().trim_end_matches(".git");
    Some(format!("https://salsa.debian.org{}", path))
}

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

fn salsa_path_from_alioth_url(vcs_type: &str, alioth_url: &str) -> Option<String> {
    let team_name_map = get_team_name_map();
    let git_path_renames = get_git_path_renames();

    if vcs_type.to_lowercase() == "git" {
        let pat = regex::Regex::new(
            r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?collab-maint/",
        )
        .ok()?;
        if pat.is_match(alioth_url) {
            return Some(pat.replace(alioth_url, "debian/").to_string());
        }
        let pat =
            regex::Regex::new(r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?users/")
                .ok()?;
        if pat.is_match(alioth_url) {
            return Some(pat.replace(alioth_url, "").to_string());
        }
        if let Some(caps) =
            regex::Regex::new(r"(https?|git)://(anonscm|git)\.debian\.org/(cgit/|git/)?(.+)")
                .ok()?
                .captures(alioth_url)
        {
            let path = caps.get(4)?.as_str();
            let parts: Vec<&str> = path.split('/').collect();
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
            if let Some(first_part) = parts.first() {
                if *first_part == "debian-in" && alioth_url.contains("fonts-") {
                    return Some(format!("fonts-team/{}", parts[1..].join("/")));
                }
                if let Some(new_name) = team_name_map.get(first_part) {
                    return Some(format!("{}/{}", new_name, parts[1..].join("/")));
                }
            }
        }
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
        if alioth_url.starts_with("svn://svn.debian.org/pkg-perl/trunk") {
            return Some(alioth_url.replace(
                "svn://svn.debian.org/pkg-perl/trunk",
                "perl-team/modules/packages",
            ));
        }
        if alioth_url.starts_with("svn://svn.debian.org/pkg-lua/packages") {
            return Some(alioth_url.replace("svn://svn.debian.org/pkg-lua/packages", "lua-team"));
        }
        if let Ok(parsed) = Url::parse(alioth_url) {
            if parsed.scheme() == "svn"
                && (parsed.host_str() == Some("svn.debian.org")
                    || parsed.host_str() == Some("anonscm.debian.org"))
            {
                let mut parts: Vec<&str> =
                    parsed.path().trim_start_matches('/').split('/').collect();
                if parts.first() == Some(&"svn") {
                    parts.remove(0);
                }
                if parts.len() == 3 && team_name_map.contains_key(parts[0]) && parts[2] == "trunk" {
                    return Some(format!("{}/{}", team_name_map[parts[0]], parts[1]));
                }
                if parts.len() == 3 && team_name_map.contains_key(parts[0]) && parts[1] == "trunk" {
                    return Some(format!("{}/{}", team_name_map[parts[0]], parts[2]));
                }
                if parts.len() == 4
                    && team_name_map.contains_key(parts[0])
                    && parts[1] == "packages"
                    && parts[3] == "trunk"
                {
                    return Some(format!("{}/{}", team_name_map[parts[0]], parts[2]));
                }
                if parts.len() == 4
                    && team_name_map.contains_key(parts[0])
                    && parts[1] == "trunk"
                    && parts[2] == "packages"
                {
                    return Some(format!("{}/{}", team_name_map[parts[0]], parts[3]));
                }
                if parts.len() > 3
                    && team_name_map.contains_key(parts[0])
                    && parts[parts.len() - 2] == "trunk"
                {
                    return Some(format!(
                        "{}/{}",
                        team_name_map[parts[0]],
                        parts[parts.len() - 1]
                    ));
                }
                if parts.len() == 3
                    && team_name_map.contains_key(parts[0])
                    && (parts[1] == "packages" || parts[1] == "unstable")
                {
                    return Some(format!("{}/{}", team_name_map[parts[0]], parts[2]));
                }
            }
        }
    }
    None
}

fn salsa_url_from_alioth_url(vcs_type: &str, alioth_url: &str) -> Option<String> {
    let mut path = salsa_path_from_alioth_url(vcs_type, alioth_url)?;
    path = path.trim_end_matches('/').to_string();
    if !path.ends_with(".git") {
        path.push_str(".git");
    }
    Some(format!("https://salsa.debian.org/{}", path))
}

struct NewRepositoryURLUnknown;

async fn find_new_urls(
    vcs_type: &str,
    vcs_url: &str,
    package: &str,
    maintainer_email: &str,
    preferences: &FixerPreferences,
) -> Result<(String, String, Option<String>), NewRepositoryURLUnknown> {
    let net_access = preferences.net_access.unwrap_or(false);

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

    let guessed_url = debian_analyzer::salsa::guess_repository_url(package, maintainer_email);
    let (new_vcs_type, new_vcs_url) = if let Some(url) = guessed_url {
        ("Git".to_string(), url.to_string())
    } else {
        let converted_url =
            salsa_url_from_alioth_url(vcs_type, vcs_url).ok_or(NewRepositoryURLUnknown)?;
        ("Git".to_string(), converted_url)
    };

    if net_access && !verify_salsa_repository(&new_vcs_url).await.unwrap_or(false) {
        return Err(NewRepositoryURLUnknown);
    }

    let vcs_browser = determine_salsa_browser_url(&new_vcs_url);
    Ok((new_vcs_type, new_vcs_url, vcs_browser))
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let control_rel = PathBuf::from("debian/control");
    let control = match ws.parsed_control() {
        Ok(c) => c,
        Err(debian_workspace::Error::NotFound) => return Ok(Vec::new()),
        Err(_) => return Ok(Vec::new()),
    };
    let Some(source) = control.source() else {
        return Ok(Vec::new());
    };
    let paragraph = source.as_deb822();

    // Pick the first VCS field present.
    let pairs: &[(&str, &str)] = &[
        ("Vcs-Git", "Git"),
        ("Vcs-Svn", "Svn"),
        ("Vcs-Bzr", "Bzr"),
        ("Vcs-Hg", "Hg"),
        ("Vcs-Cvs", "Cvs"),
    ];
    let Some((field_name, vcs_type, vcs_url)) = pairs
        .iter()
        .find_map(|(field, kind)| paragraph.get(field).map(|v| (*field, *kind, v)))
    else {
        return Ok(Vec::new());
    };
    if !is_on_obsolete_host(&vcs_url) {
        return Ok(Vec::new());
    }

    let Some(package) = paragraph.get("Source") else {
        return Ok(Vec::new());
    };
    let Some(maintainer) = paragraph.get("Maintainer") else {
        return Ok(Vec::new());
    };
    let (_, maintainer_email) = parseaddr(&maintainer);

    let old_vcs_browser = paragraph.get("Vcs-Browser");
    let old_vcs_url = vcs_url.clone();

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FixerError::Other(format!("Failed to create runtime: {}", e)))?;
    let Ok((new_vcs_type, new_vcs_url, new_vcs_browser)) = rt.block_on(find_new_urls(
        vcs_type,
        &vcs_url,
        &package,
        maintainer_email,
        preferences,
    )) else {
        return Ok(Vec::new());
    };

    // Fields to drop: every Vcs-* (and Browser unless we're keeping it)
    // that doesn't match the target Vcs-* type.
    let new_vcs_field = format!("Vcs-{}", new_vcs_type);
    let mut actions: Vec<Action> = Vec::new();
    for hdr in ["Vcs-Git", "Vcs-Bzr", "Vcs-Hg", "Vcs-Svn", "Vcs-Cvs"] {
        if hdr != new_vcs_field && paragraph.get(hdr).is_some() {
            actions.push(Action::Deb822(Deb822Action::RemoveField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: hdr.into(),
            }));
        }
    }
    if new_vcs_browser.is_none() && old_vcs_browser.is_some() {
        actions.push(Action::Deb822(Deb822Action::RemoveField {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: "Vcs-Browser".into(),
        }));
    }
    actions.push(Action::Deb822(Deb822Action::SetField {
        file: control_rel.clone(),
        paragraph: ParagraphSelector::Source,
        field: new_vcs_field.clone(),
        value: new_vcs_url.clone(),
    }));
    if let Some(browser) = new_vcs_browser.clone() {
        actions.push(Action::Deb822(Deb822Action::SetField {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: "Vcs-Browser".into(),
            value: browser,
        }));
    }

    let mut diagnostics = Vec::new();

    diagnostics.push(Diagnostic::with_actions(
        LintianIssue::source_with_info(
            "vcs-obsolete-in-debian-infrastructure",
            Visibility::Warning,
            vec![format!("vcs-{} {}", vcs_type.to_lowercase(), old_vcs_url)],
        ),
        "Vcs-* headers point to obsolete Debian infrastructure.",
        "Update Vcs-* headers to use salsa repository.",
        actions.clone(),
    ));

    // Bitrotted-tag emission: CVS via cvs.alioth/anonscm or Svn with
    // viewvc browser.
    let is_cvs_bitrotted = paragraph
        .get("Vcs-Cvs")
        .as_deref()
        .and_then(|cvs_url| {
            regex::Regex::new(r"@(?:cvs\.alioth|anonscm)\.debian\.org:/cvsroot/")
                .ok()
                .map(|re| re.is_match(cvs_url))
        })
        .unwrap_or(false);
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
        diagnostics.push(Diagnostic::with_actions(
            LintianIssue::source_with_info("vcs-field-bitrotted", Visibility::Warning, vec![info]),
            "Vcs-* field is bitrotted.",
            "Update Vcs-* headers to use salsa repository.",
            actions,
        ));
    }

    // Suppress the unused-warning when the udd cfg path makes
    // `field_name` unused.
    let _ = field_name;
    Ok(diagnostics)
}

declare_detector! {
    name: "vcs-field-bitrotted",
    tags: ["vcs-obsolete-in-debian-infrastructure", "vcs-field-bitrotted"],
    before: ["vcs-broken-uri"],
    triggers: [
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Source",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Maintainer",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Vcs-*",
        },
    ],
    cost: crate::detector::DetectorCost::Network,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::DetectorAdapter;
    use crate::Version;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply_with(
        base: &Path,
        prefs: &FixerPreferences,
    ) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, prefs)
    }

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
        let result =
            salsa_url_from_alioth_url("Git", "git://git.debian.org/pkg-javascript/node-foo");
        assert_eq!(
            result,
            Some("https://salsa.debian.org/js-team/node-foo.git".to_string())
        );
    }

    #[test]
    fn test_salsa_url_from_alioth_url_personal() {
        // Personal repos return None; the find_new_urls layer handles them
        // via guess_repository_url when network access is allowed.
        assert_eq!(
            salsa_url_from_alioth_url("Git", "git://git.debian.org/jelmer/lintian-brush"),
            None
        );
    }

    #[test]
    fn test_simple_migration() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        fs::write(
            &path,
            "Source: lintian-brush\nMaintainer: Jelmer Vernooij <jelmer@debian.org>\nVcs-Git: git://git.debian.org/jelmer/lintian-brush\nVcs-Browser: https://alioth.debian.org/git/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n",
        )
        .unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        let result = run_apply_with(tmp.path(), &prefs).unwrap();
        assert!(result
            .description
            .contains("Update Vcs-* headers to use salsa repository"));

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Vcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git"));
        assert!(after.contains("Vcs-Browser: https://salsa.debian.org/jelmer/lintian-brush"));
    }

    #[test]
    fn test_already_on_salsa() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("control");
        let original = "Source: lintian-brush\nMaintainer: Jelmer Vernooij <jelmer@debian.org>\nVcs-Git: https://salsa.debian.org/jelmer/lintian-brush.git\nVcs-Browser: https://salsa.debian.org/jelmer/lintian-brush\n\nPackage: lintian-brush\nDescription: Testing\n Test test\n";
        fs::write(&path, original).unwrap();

        let prefs = FixerPreferences {
            net_access: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            run_apply_with(tmp.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
