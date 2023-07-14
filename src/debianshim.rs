use pyo3::prelude::*;
pub struct Changelog(PyObject);
use debversion::Version;

impl Changelog {
    pub fn new(text: &str, max_blocks: Option<i32>) -> PyResult<Self> {
        Python::with_gil(|py| {
            let changelog = py.import("debian.changelog")?;
            let changelog = changelog.getattr("Changelog")?;
            let changelog = changelog.call1((text, max_blocks))?;
            Ok(Changelog(changelog.into()))
        })
    }

    pub fn pop_first(&self) -> PyResult<()> {
        Python::with_gil(|py| {
            let blocks = self.0.getattr(py, "_blocks")?;
            blocks.as_ref(py).del_item(0)?;
            Ok(())
        })
    }

    pub fn from_reader(mut r: impl std::io::Read, max_blocks: Option<i32>) -> PyResult<Self> {
        let mut text = String::new();
        r.read_to_string(&mut text)?;
        Self::new(text.as_str(), max_blocks)
    }

    pub fn package(&self) -> String {
        Python::with_gil(|py| {
            let package = self.0.getattr(py, "package").unwrap();
            package.extract(py).unwrap()
        })
    }

    pub fn version(&self) -> Version {
        Python::with_gil(|py| {
            let version = self.0.getattr(py, "version").unwrap();
            version.extract(py).unwrap()
        })
    }

    pub fn distributions(&self) -> String {
        Python::with_gil(|py| {
            let distributions = self.0.getattr(py, "distributions").unwrap();
            distributions.extract(py).unwrap()
        })
    }
}

impl ToString for Changelog {
    fn to_string(&self) -> String {
        Python::with_gil(|py| {
            let s = self.0.call_method0(py, "__str__").unwrap();
            s.extract(py).unwrap()
        })
    }
}
