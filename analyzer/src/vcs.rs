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
    vcs_type: &str,
    vcs_url: &Url,
    net_access: Option<bool>,
) -> Option<Url> {
    let parsed_vcs: ParsedVcs = vcs_url.as_str().parse().unwrap();

    let parsed_url: Url = parsed_vcs.repo_url.parse().unwrap();

    match parsed_url.host_str().unwrap() {
        host if is_gitlab_site(host, net_access) => {
            return Some(determine_gitlab_browser_url(vcs_url.as_str()));
        }

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
            let mut path_elements = parsed_url.path_segments().unwrap().collect::<Vec<_>>();
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
                path_elements.extend(["ci", &branch, "tree"]);
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

#[cfg(test)]
mod tests {
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
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar/").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar/.git").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar/").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git/").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git/.git").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar.git/").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git.git").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git.git/").unwrap(),
                Some(false)
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
    }
}
