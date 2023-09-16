use pyo3::prelude::*;
use url::Url;

pub fn guess_repository_url(package: &str, maintainer_email: &str) -> Option<Url> {
    Python::with_gil(|py| -> PyResult<Option<Url>> {
        let m = py.import("lintian_brush.salsa")?;
        let guess_repository_url = m.getattr("guess_repository_url")?;
        let url = guess_repository_url.call1((package, maintainer_email))?;
        Ok(url
            .extract::<Option<String>>()
            .unwrap()
            .map(|url| url::Url::parse(&url).unwrap()))
    })
    .unwrap()
}
