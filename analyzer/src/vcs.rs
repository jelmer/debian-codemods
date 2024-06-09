use debian_control::vcs::ParsedVcs;
use url::Url;

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

pub fn determine_browser_url(vcs_type: &str, vcs_url: &Url) -> Option<Url> {
    pyo3::Python::with_gil(|py| {
        let vcs = py.import("lintian_brush.vcs").unwrap();
        let cb = vcs.getattr("determine_browser_url").unwrap();
        let url = cb.call1((vcs_type, vcs_url.as_str())).unwrap();
        let url = url.extract::<String>().unwrap();
        let url = Url::parse(&url).ok()?;
        Some(url)
    })
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
                &Url::parse("https://salsa.debian.org/foo/bar").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar/").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar/.git").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar/").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git/").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git/.git").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar.git/").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git.git").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar.git").unwrap())
        );
        assert_eq!(
            determine_browser_url(
                "git",
                &Url::parse("https://salsa.debian.org/foo/bar.git.git/").unwrap()
            ),
            Some(Url::parse("https://salsa.debian.org/foo/bar.git").unwrap())
        );
    }
}
