use pyo3::prelude::*;

pub fn get_maintainer() -> (Option<String>, Option<String>) {
    pyo3::Python::with_gil(|py| {
        let m = py.import("debian.changelog").unwrap();
        let f = m.getattr("get_maintainer").unwrap();
        f.call((), None).unwrap().extract().unwrap()
    })
}
