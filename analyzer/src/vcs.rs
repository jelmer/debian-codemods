use url::Url;

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
