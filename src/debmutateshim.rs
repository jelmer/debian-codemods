use pyo3::prelude::*;

pub struct Deb822Paragraph(pub(crate) PyObject);

impl Deb822Paragraph {
    pub fn insert(&self, key: &str, value: &str) {
        Python::with_gil(|py| {
            self.0.call_method1(py, "insert", (key, value)).unwrap();
        })
    }

    pub fn remove(&self, key: &str) {
        Python::with_gil(|py| {
            self.0.call_method1(py, "remove", (key,)).unwrap();
        })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        Python::with_gil(|py| {
            let result = self.0.call_method1(py, "get", (key,)).unwrap();
            if result.is_none(py) {
                None
            } else {
                Some(result.extract(py).unwrap())
            }
        })
    }
}

pub struct ControlEditor(pub(crate) PyObject);

impl ControlEditor {
    pub fn source(&self) -> Option<Deb822Paragraph> {
        Python::with_gil(|py| {
            let result = self.0.call_method0(py, "source").unwrap();
            if result.is_none(py) {
                None
            } else {
                Some(Deb822Paragraph(result))
            }
        })
    }

    pub fn binaries(&self) -> Vec<Deb822Paragraph> {
        Python::with_gil(|py| {
            let result: Vec<PyObject> = self
                .0
                .call_method0(py, "binaries")
                .unwrap()
                .extract(py)
                .unwrap();
            result.into_iter().map(Deb822Paragraph).collect()
        })
    }
}
