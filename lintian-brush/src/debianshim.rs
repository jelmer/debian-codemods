use debversion::Version;
use pyo3::prelude::*;

pub struct Changelog(PyObject, Vec<ChangeBlock>);

impl Changelog {
    pub fn new(text: &str, max_blocks: Option<i32>) -> PyResult<Self> {
        Python::with_gil(|py| {
            let m = py.import("debian.changelog")?;
            let f = m.getattr("Changelog")?;
            let changelog = f.call1((text, max_blocks))?;
            Self::from_pyobject(changelog)
        })
    }

    pub fn from_pyobject(changelog: &PyAny) -> PyResult<Self> {
        let blocks = changelog.getattr("_blocks")?.extract()?;
        Ok(Changelog(changelog.into(), blocks))
    }

    pub fn pop_first(&mut self) {
        self.1.remove(0);
        Python::with_gil(|py| {
            let blocks = self.0.getattr(py, "_blocks").unwrap();
            blocks.as_ref(py).del_item(0).unwrap();
        });
    }

    pub fn from_reader(mut r: impl std::io::Read, max_blocks: Option<i32>) -> PyResult<Self> {
        let mut text = String::new();
        r.read_to_string(&mut text)?;
        Self::new(text.as_str(), max_blocks)
    }

    pub fn len(&self) -> usize {
        self.1.len()
    }

    pub fn is_empty(&self) -> bool {
        self.1.is_empty()
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

impl core::ops::Index<usize> for Changelog {
    type Output = ChangeBlock;

    fn index(&self, index: usize) -> &Self::Output {
        &self.1[index]
    }
}

pub struct ChangeBlock {
    changes: Vec<String>,
    distributions: String,
    version: Version,
    package: String,
}

impl ChangeBlock {
    pub fn changes(&self) -> &Vec<String> {
        &self.changes
    }

    pub fn distributions(&self) -> &str {
        self.distributions.as_str()
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn package(&self) -> &str {
        self.package.as_str()
    }
}

impl FromPyObject<'_> for ChangeBlock {
    fn extract(ob: &PyAny) -> PyResult<Self> {
        let changes = ob.getattr("_changes")?;
        let changes = changes.extract()?;
        let distributions = ob.getattr("distributions")?;
        let distributions = distributions.extract()?;
        let version = ob.getattr("version")?;
        let version = version.extract()?;
        let package = ob.getattr("package")?;
        let package = package.extract()?;
        Ok(ChangeBlock {
            changes,
            distributions,
            version,
            package,
        })
    }
}
