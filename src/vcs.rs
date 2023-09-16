use url::Url;

pub fn determine_browser_url(vcs_type: &str, vcs_url: &Url) -> Option<Url> {
    pyo3::Python::with_gil(|py| {
        let vcs = py.import("lintian_brush.vcs").unwrap();
        let cb = vcs.getattr("determine_browser_url").unwrap();
        let url = vcs.call1((vcs_type, vcs_url.as_str())).unwrap();
        let url = url.extract::<String>().unwrap();
        let url = Url::parse(&url).ok()?;
        Some(url)
    })
}
