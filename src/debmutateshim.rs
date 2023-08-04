use pyo3::prelude::*;
use std::path::Path;

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
    pub fn new(path: Option<&Path>) -> Self {
        Python::with_gil(|py| {
            let path = path.map_or_else(|| "debian/control", |p| p.to_str().unwrap());
            let control = py
                .import("debmutate.control")
                .unwrap()
                .call_method1("ControlEditor", (path,))
                .unwrap();
            let o = control.to_object(py);
            o.call_method0(py, "__enter__").unwrap();
            ControlEditor(o)
        })
    }

    pub fn create(path: Option<&Path>) -> Self {
        Python::with_gil(|py| {
            let path = path.map_or_else(|| "debian/control", |p| p.to_str().unwrap());
            let control = py
                .import("debmutate.control")
                .unwrap()
                .getattr("ControlEditor")
                .unwrap()
                .call_method1("create", (path,))
                .unwrap();
            let o = control.to_object(py);
            o.call_method0(py, "__enter__").unwrap();
            ControlEditor(o)
        })
    }

    pub fn source(&self) -> Option<Deb822Paragraph> {
        Python::with_gil(|py| {
            let result = self.0.getattr(py, "source").unwrap();
            if result.is_none(py) {
                None
            } else {
                Some(Deb822Paragraph(result))
            }
        })
    }

    pub fn binaries(&self) -> Vec<Deb822Paragraph> {
        Python::with_gil(|py| {
            let elements = self.0.getattr(py, "binaries").unwrap();
            let mut binaries = vec![];
            for elem in elements.as_ref(py).iter().unwrap() {
                let elem = elem.unwrap();
                binaries.push(Deb822Paragraph(elem.to_object(py)));
            }
            binaries
        })
    }
}

impl Drop for ControlEditor {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "__exit__", (py.None(), py.None(), py.None()))
                .unwrap();
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_create() {
        use super::ControlEditor;
        let td = tempfile::tempdir().unwrap();
        let ce = ControlEditor::create(Some(td.path().join("control").as_path()));
        assert!(ce.source().is_some());
        assert!(ce.binaries().is_empty());
    }
}
