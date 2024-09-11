use debian_control::vcs::ParsedVcs;
use log::debug;
use url::Url;

pub const KNOWN_GITLAB_SITES: &[&str] = &["salsa.debian.org", "invent.kde.org", "0xacab.org"];

pub fn is_gitlab_site(hostname: &str, net_access: Option<bool>) -> bool {
    if KNOWN_GITLAB_SITES.contains(&hostname) {
        return true;
    }

    if hostname.starts_with("gitlab.") {
        return true;
    }

    if net_access.unwrap_or(false) {
        probe_gitlab_host(hostname)
    } else {
        false
    }
}

pub fn probe_gitlab_host(hostname: &str) -> bool {
    use reqwest::header::HeaderMap;
    let url = format!("https://{}/api/v4/version", hostname);

    let mut headers = HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());

    let client = reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();

    let http_url: reqwest::Url = Into::<String>::into(url.clone()).parse().unwrap();

    let request = client.get(http_url).build().unwrap();

    let response = client.execute(request).unwrap();

    match response.status().as_u16() {
        401 => {
            if let Ok(data) = response.json::<serde_json::Value>() {
                if let Some(message) = data["message"].as_str() {
                    if message == "401 Unauthorized" {
                        true
                    } else {
                        debug!("failed to parse JSON response: {:?}", data);
                        false
                    }
                } else {
                    debug!("failed to parse JSON response: {:?}", data);
                    false
                }
            } else {
                debug!("failed to parse JSON response");
                false
            }
        }
        200 => true,
        _ => {
            debug!("unexpected HTTP status code: {:?}", response.status());
            false
        }
    }
}

pub fn determine_gitlab_browser_url(url: &str) -> Url {
    let parsed_vcs: ParsedVcs = url.trim_end_matches('/').parse().unwrap();

    // TODO(jelmer): Add support for branches
    let parsed_url = Url::parse(&parsed_vcs.repo_url).unwrap();

    let path = parsed_url
        .path()
        .trim_end_matches('/')
        .trim_end_matches(".git");

    let branch = if let Some(branch) = parsed_vcs.branch {
        Some(branch)
    } else if parsed_vcs.subpath.is_some() {
        Some("HEAD".to_string())
    } else {
        None
    };

    let mut path = if let Some(branch) = branch {
        format!("{}/-/tree/{}", path, branch)
    } else {
        path.to_string()
    };

    if let Some(subpath) = parsed_vcs.subpath {
        path.push_str(&format!("/{}", subpath));
    }

    let url = format!(
        "https://{}/{}",
        parsed_url.host_str().unwrap(),
        path.trim_start_matches('/')
    );

    Url::parse(&url).unwrap()
}

pub fn determine_browser_url(
    _vcs_type: &str,
    vcs_url: &str,
    net_access: Option<bool>,
) -> Option<Url> {
    let parsed_vcs: ParsedVcs = vcs_url.parse().unwrap();

    let parsed_url: Url = parsed_vcs.repo_url.parse().unwrap();

    match parsed_url.host_str().unwrap() {
        host if is_gitlab_site(host, net_access) => Some(determine_gitlab_browser_url(vcs_url)),

        "github.com" => {
            let path = parsed_url.path().trim_end_matches(".git");

            let branch = if let Some(branch) = parsed_vcs.branch {
                Some(branch)
            } else if parsed_vcs.subpath.is_some() {
                Some("HEAD".to_string())
            } else {
                None
            };

            let mut path = if let Some(branch) = branch {
                format!("{}/tree/{}", path, branch)
            } else {
                path.to_string()
            };

            if let Some(subpath) = parsed_vcs.subpath {
                path.push_str(&format!("/{}", subpath));
            }

            let url = format!(
                "https://{}/{}",
                parsed_url.host_str().unwrap(),
                path.trim_start_matches('/')
            );

            Some(Url::parse(&url).unwrap())
        }
        host if (host == "code.launchpad.net" || host == "launchpad.net")
            && parsed_vcs.branch.is_none()
            && parsed_vcs.subpath.is_none() =>
        {
            let url = format!(
                "https://code.launchpad.net/{}",
                parsed_url.path().trim_start_matches('/')
            );

            Some(Url::parse(&url).unwrap())
        }
        "git.savannah.gnu.org" | "git.sv.gnu.org" => {
            let mut path_elements = parsed_url.path_segments().unwrap().collect::<Vec<_>>();
            if parsed_url.scheme() == "https" && path_elements.first() == Some(&"git") {
                path_elements.remove(0);
            }
            // Why cgit and not gitweb?
            path_elements.insert(0, "cgit");
            Some(
                Url::parse(&format!(
                    "https://{}/{}",
                    parsed_url.host_str().unwrap(),
                    path_elements.join("/")
                ))
                .unwrap(),
            )
        }
        "git.code.sf.net" | "git.code.sourceforge.net" => {
            let path_elements = parsed_url.path_segments().unwrap().collect::<Vec<_>>();
            if path_elements.first() != Some(&"p") {
                return None;
            }
            let project = path_elements[1];
            let repository = path_elements[2];
            let mut path_elements = vec!["p", project, repository];
            let branch = if let Some(branch) = parsed_vcs.branch {
                Some(branch)
            } else if parsed_vcs.subpath.is_some() {
                Some("HEAD".to_string())
            } else {
                None
            };

            if let Some(branch) = branch.as_deref() {
                path_elements.extend(["ci", branch, "tree"]);
            }

            if let Some(subpath) = parsed_vcs.subpath.as_ref() {
                path_elements.push(subpath);
            }

            let url = format!("https://sourceforge.net/{}", path_elements.join("/"));
            Some(Url::parse(&url).unwrap())
        }
        _ => None,
    }
}

pub fn canonicalize_vcs_browser_url(url: &str) -> String {
    let url = url.replace(
        "https://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/",
    );
    let url = url.replace(
        "http://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/",
    );
    let url = url.replace(
        "https://git.debian.org/?p=",
        "https://anonscm.debian.org/git/",
    );
    let url = url.replace(
        "http://git.debian.org/?p=",
        "https://anonscm.debian.org/git/",
    );
    let url = url.replace(
        "https://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/",
    );
    let url = url.replace(
        "http://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/",
    );

    lazy_regex::regex_replace!(
        r"^https?://salsa.debian.org/([^/]+/[^/]+)\.git/?$",
        &url,
        |_, x| "https://salsa.debian.org/".to_string() + x
    )
    .into_owned()
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PackageVcs {
    Git {
        url: Url,
        branch: Option<String>,
        subpath: Option<std::path::PathBuf>,
    },
    Svn(Url),
    Bzr(Url),
    Hg {
        url: Url,
        branch: Option<String>,
        subpath: Option<std::path::PathBuf>,
    },
    Mtn(Url),
    Cvs(String),
    Darcs(Url),
    Arch(Url),
    Svk(Url),
}

impl PackageVcs {
    pub fn type_str(&self) -> &str {
        match self {
            PackageVcs::Git { .. } => "Git",
            PackageVcs::Svn(_) => "Svn",
            PackageVcs::Bzr(_) => "Bzr",
            PackageVcs::Hg { .. } => "Hg",
            PackageVcs::Mtn(_) => "Mtn",
            PackageVcs::Cvs(_) => "Cvs",
            PackageVcs::Darcs(_) => "Darcs",
            PackageVcs::Arch(_) => "Arch",
            PackageVcs::Svk(_) => "Svk",
        }
    }

    pub fn url(&self) -> Option<&url::Url> {
        match self {
            PackageVcs::Git { url, .. } => Some(url),
            PackageVcs::Svn(url) => Some(url),
            PackageVcs::Bzr(url) => Some(url),
            PackageVcs::Hg { url, .. } => Some(url),
            PackageVcs::Mtn(url) => Some(url),
            PackageVcs::Darcs(url) => Some(url),
            PackageVcs::Arch(url) => Some(url),
            PackageVcs::Svk(url) => Some(url),
            PackageVcs::Cvs(_) => None,
        }
    }

    pub fn branch(&self) -> Option<&str> {
        match self {
            PackageVcs::Git { branch, .. } => branch.as_deref(),
            PackageVcs::Hg { branch, .. } => branch.as_deref(),
            _ => None,
        }
    }

    pub fn subpath(&self) -> Option<&std::path::Path> {
        match self {
            PackageVcs::Git { subpath, .. } => subpath.as_deref(),
            PackageVcs::Hg { subpath, .. } => subpath.as_deref(),
            _ => None,
        }
    }

    pub fn location(&self) -> String {
        match self {
            PackageVcs::Git {
                url,
                branch,
                subpath,
            } => {
                let mut result = url.to_string();
                if let Some(branch) = branch {
                    result.push_str(&format!(" -b {}", branch));
                }
                if let Some(subpath) = subpath {
                    result.push_str(&format!(" [{}]", subpath.display()));
                }
                result
            }
            PackageVcs::Svn(url) => url.to_string(),
            PackageVcs::Bzr(url) => url.to_string(),
            PackageVcs::Hg {
                url,
                branch,
                subpath,
            } => {
                let mut result = url.to_string();
                if let Some(branch) = branch {
                    result.push_str(&format!(" -b {}", branch));
                }
                if let Some(subpath) = subpath {
                    result.push_str(&format!(" [{}]", subpath.display()));
                }
                result
            }
            PackageVcs::Mtn(url) => url.to_string(),
            PackageVcs::Cvs(s) => s.clone(),
            PackageVcs::Darcs(url) => url.to_string(),
            PackageVcs::Arch(url) => url.to_string(),
            PackageVcs::Svk(url) => url.to_string(),
        }
    }
}

impl From<PackageVcs> for ParsedVcs {
    fn from(vcs: PackageVcs) -> Self {
        match vcs {
            PackageVcs::Git {
                url,
                branch,
                subpath,
            } => ParsedVcs {
                repo_url: url.to_string(),
                branch,
                subpath: subpath.map(|x| x.to_string_lossy().to_string()),
            },
            PackageVcs::Svn(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
            PackageVcs::Bzr(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
            PackageVcs::Hg {
                url,
                branch,
                subpath,
            } => ParsedVcs {
                repo_url: url.to_string(),
                branch,
                subpath: subpath.map(|x| x.to_string_lossy().to_string()),
            },
            PackageVcs::Mtn(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
            PackageVcs::Cvs(s) => ParsedVcs {
                repo_url: s,
                branch: None,
                subpath: None,
            },
            PackageVcs::Darcs(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
            PackageVcs::Arch(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
            PackageVcs::Svk(url) => ParsedVcs {
                repo_url: url.to_string(),
                branch: None,
                subpath: None,
            },
        }
    }
}

pub trait VcsSource {
    fn vcs_git(&self) -> Option<String>;
    fn vcs_svn(&self) -> Option<String>;
    fn vcs_bzr(&self) -> Option<String>;
    fn vcs_hg(&self) -> Option<String>;
    fn vcs_mtn(&self) -> Option<String>;
    fn vcs_cvs(&self) -> Option<String>;
    fn vcs_darcs(&self) -> Option<String>;
    fn vcs_arch(&self) -> Option<String>;
    fn vcs_svk(&self) -> Option<String>;
}

impl VcsSource for debian_control::Source {
    fn vcs_git(&self) -> Option<String> {
        self.vcs_git()
    }

    fn vcs_svn(&self) -> Option<String> {
        self.vcs_svn()
    }

    fn vcs_bzr(&self) -> Option<String> {
        self.vcs_bzr()
    }

    fn vcs_hg(&self) -> Option<String> {
        self.vcs_hg()
    }

    fn vcs_mtn(&self) -> Option<String> {
        self.vcs_mtn()
    }

    fn vcs_cvs(&self) -> Option<String> {
        self.vcs_cvs()
    }

    fn vcs_darcs(&self) -> Option<String> {
        self.vcs_darcs()
    }

    fn vcs_arch(&self) -> Option<String> {
        self.vcs_arch()
    }

    fn vcs_svk(&self) -> Option<String> {
        self.vcs_svk()
    }
}

impl VcsSource for debian_control::apt::Source {
    fn vcs_git(&self) -> Option<String> {
        self.vcs_git()
    }

    fn vcs_svn(&self) -> Option<String> {
        self.vcs_svn()
    }

    fn vcs_bzr(&self) -> Option<String> {
        self.vcs_bzr()
    }

    fn vcs_hg(&self) -> Option<String> {
        self.vcs_hg()
    }

    fn vcs_mtn(&self) -> Option<String> {
        self.vcs_mtn()
    }

    fn vcs_cvs(&self) -> Option<String> {
        self.vcs_cvs()
    }

    fn vcs_darcs(&self) -> Option<String> {
        self.vcs_darcs()
    }

    fn vcs_arch(&self) -> Option<String> {
        self.vcs_arch()
    }

    fn vcs_svk(&self) -> Option<String> {
        self.vcs_svk()
    }
}

pub fn vcs_field(source_package: &impl VcsSource) -> Option<(String, String)> {
    if let Some(value) = source_package.vcs_git() {
        return Some(("Git".to_string(), value));
    }
    if let Some(value) = source_package.vcs_svn() {
        return Some(("Svn".to_string(), value));
    }
    if let Some(value) = source_package.vcs_bzr() {
        return Some(("Bzr".to_string(), value));
    }
    if let Some(value) = source_package.vcs_hg() {
        return Some(("Hg".to_string(), value));
    }
    if let Some(value) = source_package.vcs_mtn() {
        return Some(("Mtn".to_string(), value));
    }
    if let Some(value) = source_package.vcs_cvs() {
        return Some(("Cvs".to_string(), value));
    }
    if let Some(value) = source_package.vcs_darcs() {
        return Some(("Darcs".to_string(), value));
    }
    if let Some(value) = source_package.vcs_arch() {
        return Some(("Arch".to_string(), value));
    }
    if let Some(value) = source_package.vcs_svk() {
        return Some(("Svk".to_string(), value));
    }
    None
}

pub fn source_package_vcs(source_package: &impl VcsSource) -> Option<PackageVcs> {
    if let Some(value) = source_package.vcs_git() {
        let parsed_vcs: ParsedVcs = value.parse().unwrap();
        let url = parsed_vcs.repo_url.parse().unwrap();
        return Some(PackageVcs::Git {
            url,
            branch: parsed_vcs.branch,
            subpath: parsed_vcs.subpath.map(std::path::PathBuf::from),
        });
    }
    if let Some(value) = source_package.vcs_svn() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Svn(url));
    }
    if let Some(value) = source_package.vcs_bzr() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Bzr(url));
    }
    if let Some(value) = source_package.vcs_hg() {
        let parsed_vcs: ParsedVcs = value.parse().unwrap();
        let url = parsed_vcs.repo_url.parse().unwrap();
        return Some(PackageVcs::Hg {
            url,
            branch: parsed_vcs.branch,
            subpath: parsed_vcs.subpath.map(std::path::PathBuf::from),
        });
    }
    if let Some(value) = source_package.vcs_mtn() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Mtn(url));
    }
    if let Some(value) = source_package.vcs_cvs() {
        return Some(PackageVcs::Cvs(value.clone()));
    }
    if let Some(value) = source_package.vcs_darcs() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Darcs(url));
    }
    if let Some(value) = source_package.vcs_arch() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Arch(url));
    }
    if let Some(value) = source_package.vcs_svk() {
        let url = value.parse().unwrap();
        return Some(PackageVcs::Svk(url));
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_source_package_vcs() {
        use super::PackageVcs;
        use debian_control::Control;

        let control: Control = r#"Source: foo
Vcs-Git: https://salsa.debian.org/foo/bar.git
"#
        .parse()
        .unwrap();
        assert_eq!(
            super::source_package_vcs(&control.source().unwrap()),
            Some(PackageVcs::Git {
                url: "https://salsa.debian.org/foo/bar.git".parse().unwrap(),
                branch: None,
                subpath: None
            })
        );

        let control: Control = r#"Source: foo
Vcs-Svn: https://svn.debian.org/svn/foo/bar
"#
        .parse()
        .unwrap();
        assert_eq!(
            super::source_package_vcs(&control.source().unwrap()),
            Some(PackageVcs::Svn(
                "https://svn.debian.org/svn/foo/bar".parse().unwrap()
            ))
        );
    }

    #[test]
    fn test_determine_gitlab_browser_url() {
        use super::determine_gitlab_browser_url;

        assert_eq!(
            determine_gitlab_browser_url("https://salsa.debian.org/foo/bar"),
            "https://salsa.debian.org/foo/bar".parse().unwrap()
        );

        assert_eq!(
            determine_gitlab_browser_url("https://salsa.debian.org/foo/bar.git"),
            "https://salsa.debian.org/foo/bar".parse().unwrap()
        );

        assert_eq!(
            determine_gitlab_browser_url("https://salsa.debian.org/foo/bar/"),
            "https://salsa.debian.org/foo/bar".parse().unwrap()
        );

        assert_eq!(
            determine_gitlab_browser_url("https://salsa.debian.org/foo/bar/.git"),
            "https://salsa.debian.org/foo/bar/".parse().unwrap()
        );

        assert_eq!(
            determine_gitlab_browser_url("https://salsa.debian.org/foo/bar.git -b baz"),
            "https://salsa.debian.org/foo/bar/-/tree/baz"
                .parse()
                .unwrap()
        );

        assert_eq!(
            determine_gitlab_browser_url(
                "https://salsa.debian.org/foo/bar.git/ -b baz [otherpath]"
            ),
            "https://salsa.debian.org/foo/bar/-/tree/baz/otherpath"
                .parse()
                .unwrap()
        );
    }

    #[test]
    fn test_determine_browser_url() {
        use super::determine_browser_url;
        use url::Url;

        assert_eq!(
            determine_browser_url("git", "https://salsa.debian.org/foo/bar", Some(false)),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url("git", "https://salsa.debian.org/foo/bar.git", Some(false)),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url("git", "https://salsa.debian.org/foo/bar/", Some(false)),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url("git", "https://salsa.debian.org/foo/bar/.git", Some(false)),
            Some(Url::parse("https://salsa.debian.org/foo/bar/").unwrap())
        );
        assert_eq!(
            determine_browser_url("git", "https://salsa.debian.org/foo/bar.git/", Some(false)),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                "https://salsa.debian.org/foo/bar.git/.git",
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar.git/").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                "https://salsa.debian.org/foo/bar.git.git",
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                "https://salsa.debian.org/foo/bar.git.git/",
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );

        assert_eq!(
            Some(Url::parse("https://salsa.debian.org/jelmer/dulwich").unwrap()),
            determine_browser_url(
                "git",
                "https://salsa.debian.org/jelmer/dulwich.git",
                Some(false)
            )
        );

        assert_eq!(
            Some(Url::parse("https://github.com/jelmer/dulwich").unwrap()),
            determine_browser_url("git", "https://github.com/jelmer/dulwich.git", Some(false))
        );
        assert_eq!(
            Some(Url::parse("https://github.com/jelmer/dulwich/tree/master").unwrap()),
            determine_browser_url(
                "git",
                "https://github.com/jelmer/dulwich.git -b master",
                Some(false)
            )
        );
        assert_eq!(
            Some(Url::parse("https://github.com/jelmer/dulwich/tree/master").unwrap()),
            determine_browser_url(
                "git",
                "git://github.com/jelmer/dulwich -b master",
                Some(false)
            ),
        );
        assert_eq!(
            Some(Url::parse("https://github.com/jelmer/dulwich/tree/master/blah").unwrap()),
            determine_browser_url(
                "git",
                "git://github.com/jelmer/dulwich -b master [blah]",
                Some(false)
            ),
        );
        assert_eq!(
            Some(Url::parse("https://github.com/jelmer/dulwich/tree/HEAD/blah").unwrap()),
            determine_browser_url("git", "git://github.com/jelmer/dulwich [blah]", Some(false)),
        );
        assert_eq!(
            Some(Url::parse("https://git.sv.gnu.org/cgit/rcs.git").unwrap()),
            determine_browser_url("git", "https://git.sv.gnu.org/git/rcs.git", Some(false)),
        );
        assert_eq!(
            Some(Url::parse("https://git.savannah.gnu.org/cgit/rcs.git").unwrap()),
            determine_browser_url("git", "git://git.savannah.gnu.org/rcs.git", Some(false)),
        );
        assert_eq!(
            Some(Url::parse("https://sourceforge.net/p/shorewall/debian").unwrap()),
            determine_browser_url(
                "git",
                "git://git.code.sf.net/p/shorewall/debian",
                Some(false)
            ),
        );
        assert_eq!(
            Some(Url::parse("https://sourceforge.net/p/shorewall/debian/ci/foo/tree").unwrap()),
            determine_browser_url(
                "git",
                "git://git.code.sf.net/p/shorewall/debian -b foo",
                Some(false)
            ),
        );
        assert_eq!(
            Some(Url::parse("https://sourceforge.net/p/shorewall/debian/ci/HEAD/tree/sp").unwrap()),
            determine_browser_url(
                "git",
                "git://git.code.sf.net/p/shorewall/debian [sp]",
                Some(false)
            ),
        );
        assert_eq!(
            Some(Url::parse("https://sourceforge.net/p/shorewall/debian/ci/foo/tree/sp").unwrap()),
            determine_browser_url(
                "git",
                "git://git.code.sf.net/p/shorewall/debian -b foo [sp]",
                Some(false)
            ),
        );
    }

    #[test]
    fn test_vcs_field() {
        use debian_control::Control;

        let control: Control = r#"Source: foo
Vcs-Git: https://salsa.debian.org/foo/bar.git
"#
        .parse()
        .unwrap();
        assert_eq!(
            super::vcs_field(&control.source().unwrap()),
            Some((
                "Git".to_string(),
                "https://salsa.debian.org/foo/bar.git".to_string()
            ))
        );
    }
}
