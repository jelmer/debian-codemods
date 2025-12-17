// Copyright (C) 2025 Jelmer Vernooij
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

//! PyO3-based wrapper around lintian-brush yaml.py for YAML editing
//!
//! This crate provides a Rust API that mimics the yaml-edit crate interface
//! but internally uses PyO3 to call the Python implementation in lintian-brush.

use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyListMethods, PyModule};
use std::path::Path;

/// Error type for YAML operations
#[derive(Debug)]
pub enum Error {
    /// Python error occurred during YAML operation
    Python(PyErr),
    /// File I/O error
    Io(std::io::Error),
    /// Value not found or wrong type
    ValueError(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Python(e) => write!(f, "Python error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::ValueError(s) => write!(f, "Value error: {}", s),
        }
    }
}

impl std::error::Error for Error {}

impl From<PyErr> for Error {
    fn from(err: PyErr) -> Self {
        Error::Python(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Initialize the Python interpreter and import required modules
fn ensure_python_initialized() -> PyResult<()> {
    Python::attach(|py| {
        // Import sys module and add py directory to path
        let sys = PyModule::import(py, "sys")?;
        let path_attr = sys.getattr("path")?;
        let path = path_attr
            .cast::<PyList>()
            .expect("sys.path should be a list");

        // Get the path to the py directory relative to this crate
        let py_path = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("py");

        if py_path.exists() {
            path.insert(0, py_path.to_str().unwrap())?;
        }

        Ok(())
    })
}

/// A YAML document editor that uses Python's YamlUpdater
pub struct YamlUpdater {
    path: std::path::PathBuf,
    remove_empty: bool,
    allow_duplicate_keys: bool,
    py_updater: Option<Py<PyAny>>,
}

impl YamlUpdater {
    /// Create a new YamlUpdater for the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        ensure_python_initialized()?;

        Ok(YamlUpdater {
            path: path.as_ref().to_path_buf(),
            remove_empty: true,
            allow_duplicate_keys: false,
            py_updater: None,
        })
    }

    /// Set whether to remove the file if it becomes empty
    pub fn remove_empty(mut self, remove_empty: bool) -> Self {
        self.remove_empty = remove_empty;
        self
    }

    /// Set whether to allow duplicate keys
    pub fn allow_duplicate_keys(mut self, allow: bool) -> Self {
        self.allow_duplicate_keys = allow;
        self
    }

    /// Enter the context manager and load the YAML file
    pub fn open(&mut self) -> Result<YamlDocument> {
        Python::attach(|py| {
            let yaml_module = PyModule::import(py, "lintian_brush.yaml")?;
            let updater_class = yaml_module.getattr("YamlUpdater")?;

            let kwargs = PyDict::new(py);
            kwargs.set_item("path", self.path.to_str().unwrap())?;
            kwargs.set_item("remove_empty", self.remove_empty)?;
            kwargs.set_item("allow_duplicate_keys", self.allow_duplicate_keys)?;

            let updater = updater_class.call((), Some(&kwargs))?;
            updater.call_method0("__enter__")?;

            self.py_updater = Some(updater.unbind());

            Ok(YamlDocument {
                updater: self.py_updater.as_ref().unwrap().clone_ref(py),
            })
        })
    }

    /// Close the context manager and save changes
    pub fn close(&mut self) -> Result<()> {
        if let Some(updater) = self.py_updater.take() {
            Python::attach(|py| {
                let updater = updater.bind(py);
                updater.call_method1("__exit__", (py.None(), py.None(), py.None()))?;
                Ok(())
            })
        } else {
            Ok(())
        }
    }
}

impl Drop for YamlUpdater {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// A YAML document that can be edited
pub struct YamlDocument {
    updater: Py<PyAny>,
}

impl YamlDocument {
    /// Get the code property (the YAML content as a Python dict)
    pub fn code(&self) -> Result<Py<PyAny>> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            let code = updater.getattr("code")?;
            Ok(code.unbind())
        })
    }

    /// Set the code property
    pub fn set_code(&self, value: Py<PyAny>) -> Result<()> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            updater.setattr("code", value)?;
            Ok(())
        })
    }

    /// Get a value from the YAML document by key
    pub fn get(&self, key: &str) -> Result<Option<Py<PyAny>>> {
        Python::attach(|py| {
            let code = self.code()?;
            let dict = code
                .bind(py)
                .cast::<PyDict>()
                .expect("YAML code should be a dict");

            if let Some(value) = dict.get_item(key)? {
                Ok(Some(value.unbind()))
            } else {
                Ok(None)
            }
        })
    }

    /// Set a value in the YAML document
    pub fn set(&self, key: &str, value: Py<PyAny>) -> Result<()> {
        Python::attach(|py| {
            // Access the updater's code attribute directly and use __setitem__
            let updater = self.updater.bind(py);
            let code = updater.getattr("code")?;
            code.call_method1("__setitem__", (key, value.bind(py)))?;
            Ok(())
        })
    }

    /// Remove a key from the YAML document
    pub fn remove(&self, key: &str) -> Result<Option<Py<PyAny>>> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            let code = updater.getattr("code")?;

            // Check if key exists
            match code.call_method1("__contains__", (key,)) {
                Ok(contains) if contains.extract::<bool>()? => {
                    let value = code.call_method1("__getitem__", (key,))?.unbind();
                    code.call_method1("__delitem__", (key,))?;
                    Ok(Some(value))
                }
                _ => Ok(None),
            }
        })
    }

    /// Check if a key exists in the YAML document
    pub fn contains_key(&self, key: &str) -> Result<bool> {
        Python::attach(|py| {
            let code = self.code()?;
            let dict = code
                .bind(py)
                .cast::<PyDict>()
                .expect("YAML code should be a dict");
            Ok(dict.contains(key)?)
        })
    }

    /// Force a rewrite of the entire YAML file
    pub fn force_rewrite(&self) -> Result<()> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            updater.call_method0("force_rewrite")?;
            Ok(())
        })
    }

    /// Get the underlying Python object for direct manipulation
    pub fn as_py_object(&self) -> &Py<PyAny> {
        &self.updater
    }
}

/// A multi-document YAML editor
pub struct MultiYamlUpdater {
    path: std::path::PathBuf,
    remove_empty: bool,
    py_updater: Option<Py<PyAny>>,
}

impl MultiYamlUpdater {
    /// Create a new MultiYamlUpdater for the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        ensure_python_initialized()?;

        Ok(MultiYamlUpdater {
            path: path.as_ref().to_path_buf(),
            remove_empty: false,
            py_updater: None,
        })
    }

    /// Set whether to remove the file if it becomes empty
    pub fn remove_empty(mut self, remove_empty: bool) -> Self {
        self.remove_empty = remove_empty;
        self
    }

    /// Enter the context manager and load the YAML file
    pub fn open(&mut self) -> Result<MultiYamlDocument> {
        Python::attach(|py| {
            let yaml_module = PyModule::import(py, "lintian_brush.yaml")?;
            let updater_class = yaml_module.getattr("MultiYamlUpdater")?;

            let kwargs = PyDict::new(py);
            kwargs.set_item("path", self.path.to_str().unwrap())?;
            kwargs.set_item("remove_empty", self.remove_empty)?;

            let updater = updater_class.call((), Some(&kwargs))?;
            updater.call_method0("__enter__")?;

            self.py_updater = Some(updater.unbind());

            Ok(MultiYamlDocument {
                updater: self.py_updater.as_ref().unwrap().clone_ref(py),
            })
        })
    }

    /// Close the context manager and save changes
    pub fn close(&mut self) -> Result<()> {
        if let Some(updater) = self.py_updater.take() {
            Python::attach(|py| {
                let updater = updater.bind(py);
                updater.call_method1("__exit__", (py.None(), py.None(), py.None()))?;
                Ok(())
            })
        } else {
            Ok(())
        }
    }
}

impl Drop for MultiYamlUpdater {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// A multi-document YAML file that can be edited
pub struct MultiYamlDocument {
    updater: Py<PyAny>,
}

impl MultiYamlDocument {
    /// Get the code property (the YAML content as a Python list)
    pub fn code(&self) -> Result<Py<PyAny>> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            let code = updater.getattr("code")?;
            Ok(code.unbind())
        })
    }

    /// Set the code property
    pub fn set_code(&self, value: Py<PyAny>) -> Result<()> {
        Python::attach(|py| {
            let updater = self.updater.bind(py);
            updater.setattr("code", value)?;
            Ok(())
        })
    }

    /// Get the number of documents
    pub fn len(&self) -> Result<usize> {
        Python::attach(|py| {
            let code = self.code()?;
            let list = code
                .bind(py)
                .cast::<PyList>()
                .expect("Multi-YAML code should be a list");
            Ok(list.len())
        })
    }

    /// Check if there are no documents
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Get a document by index
    pub fn get(&self, index: usize) -> Result<Option<Py<PyAny>>> {
        Python::attach(|py| {
            let code = self.code()?;
            let list = code
                .bind(py)
                .cast::<PyList>()
                .expect("Multi-YAML code should be a list");

            if index < list.len() {
                Ok(Some(list.get_item(index)?.unbind()))
            } else {
                Ok(None)
            }
        })
    }

    /// Set a document at the given index
    pub fn set(&self, index: usize, value: Py<PyAny>) -> Result<()> {
        Python::attach(|py| {
            let code = self.code()?;
            let list = code
                .bind(py)
                .cast::<PyList>()
                .expect("Multi-YAML code should be a list");

            if index < list.len() {
                list.set_item(index, value)?;
                Ok(())
            } else {
                Err(Error::ValueError(format!("Index {} out of bounds", index)))
            }
        })
    }

    /// Append a document to the list
    pub fn append(&self, value: Py<PyAny>) -> Result<()> {
        Python::attach(|py| {
            let code = self.code()?;
            let list = code
                .bind(py)
                .cast::<PyList>()
                .expect("Multi-YAML code should be a list");
            list.append(value)?;
            Ok(())
        })
    }

    /// Remove a document at the given index
    pub fn remove(&self, index: usize) -> Result<Py<PyAny>> {
        Python::attach(|py| {
            let code = self.code()?;
            let list = code
                .bind(py)
                .cast::<PyList>()
                .expect("Multi-YAML code should be a list");

            if index < list.len() {
                let value = list.get_item(index)?;
                let result = value.unbind();
                list.del_item(index)?;
                Ok(result)
            } else {
                Err(Error::ValueError(format!("Index {} out of bounds", index)))
            }
        })
    }

    /// Get the underlying Python object for direct manipulation
    pub fn as_py_object(&self) -> &Py<PyAny> {
        &self.updater
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_yaml_updater_get_set() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        // Create initial YAML file
        fs::write(&yaml_path, "key1: value1\nkey2: value2\n").unwrap();

        // Test reading
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let value = doc.get("key1").unwrap();
            assert!(value.is_some());

            assert!(doc.contains_key("key1").unwrap());
            assert!(!doc.contains_key("key3").unwrap());
        }

        // Test writing
        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            Python::attach(|py| {
                doc.set(
                    "key3",
                    "value3".into_pyobject(py).unwrap().into_any().unbind(),
                )
                .unwrap();
            });

            updater.close().unwrap();
        }

        // Verify changes
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("key3"));
    }

    #[test]
    fn test_yaml_updater_remove() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\nkey2: value2\nkey3: value3\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Remove key2
            let removed = doc.remove("key2").unwrap();
            assert!(removed.is_some());

            // Try to remove non-existent key
            let not_found = doc.remove("key999").unwrap();
            assert!(not_found.is_none());

            // Verify key2 is gone
            assert!(!doc.contains_key("key2").unwrap());
            assert!(doc.contains_key("key1").unwrap());
            assert!(doc.contains_key("key3").unwrap());

            updater.close().unwrap();
        }

        // Verify file doesn't contain key2
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(!content.contains("key2"));
        assert!(content.contains("key1"));
        assert!(content.contains("key3"));
    }

    #[test]
    fn test_yaml_updater_force_rewrite() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            doc.force_rewrite().unwrap();

            Python::attach(|py| {
                doc.set(
                    "key2",
                    "value2".into_pyobject(py).unwrap().into_any().unbind(),
                )
                .unwrap();
            });

            updater.close().unwrap();
        }

        // Verify file was written
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("key2"));
    }

    #[test]
    fn test_yaml_updater_remove_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap().remove_empty(true);
            let doc = updater.open().unwrap();

            // Remove all keys
            doc.remove("key1").unwrap();

            // Set empty dict
            Python::attach(|py| {
                let empty_dict = pyo3::types::PyDict::new(py);
                doc.set_code(empty_dict.into()).unwrap();
            });

            updater.close().unwrap();
        }

        // File should be deleted
        assert!(!yaml_path.exists());
    }

    #[test]
    fn test_yaml_updater_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("new.yaml");

        // File doesn't exist yet
        assert!(!yaml_path.exists());

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            Python::attach(|py| {
                doc.set(
                    "newkey",
                    "newvalue".into_pyobject(py).unwrap().into_any().unbind(),
                )
                .unwrap();
            });

            updater.close().unwrap();
        }

        // File should now exist
        assert!(yaml_path.exists());
        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("newkey"));
    }

    #[test]
    fn test_yaml_updater_code_property() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "key1: value1\n").unwrap();

        {
            let mut updater = YamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            // Get code and modify it directly
            Python::attach(|py| {
                let _code = doc.code().unwrap();
                let new_dict = pyo3::types::PyDict::new(py);
                new_dict.set_item("replaced", "yes").unwrap();
                doc.set_code(new_dict.into()).unwrap();
            });

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("replaced"));
        assert!(!content.contains("key1"));
    }

    #[test]
    fn test_multi_yaml_updater_basic() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        // Create initial multi-doc YAML file
        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 2);
            assert!(!doc.is_empty().unwrap());

            let first = doc.get(0).unwrap();
            assert!(first.is_some());

            let second = doc.get(1).unwrap();
            assert!(second.is_some());

            // Out of bounds
            let none = doc.get(5).unwrap();
            assert!(none.is_none());
        }
    }

    #[test]
    fn test_multi_yaml_updater_set() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            Python::attach(|py| {
                let new_dict = pyo3::types::PyDict::new(py);
                new_dict.set_item("doc", 999).unwrap();
                doc.set(0, new_dict.into()).unwrap();
            });

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("999"));
    }

    #[test]
    fn test_multi_yaml_updater_append() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 2);

            Python::attach(|py| {
                let new_dict = pyo3::types::PyDict::new(py);
                new_dict.set_item("doc", 3).unwrap();
                doc.append(new_dict.into()).unwrap();
            });

            assert_eq!(doc.len().unwrap(), 3);

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("doc: 3"));
    }

    #[test]
    fn test_multi_yaml_updater_remove() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n---\ndoc: 2\n---\ndoc: 3\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 3);

            let removed = doc.remove(1).unwrap();
            Python::attach(|py| {
                let dict = removed
                    .bind(py)
                    .cast::<pyo3::types::PyDict>()
                    .expect("should be dict");
                let val = dict.get_item("doc").unwrap().unwrap();
                let num: i32 = val.extract().unwrap();
                assert_eq!(num, 2);
            });

            assert_eq!(doc.len().unwrap(), 2);

            updater.close().unwrap();
        }

        let content = fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("doc: 1"));
        assert!(!content.contains("doc: 2"));
        assert!(content.contains("doc: 3"));
    }

    #[test]
    fn test_multi_yaml_updater_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("empty.yaml");

        fs::write(&yaml_path, "").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            assert_eq!(doc.len().unwrap(), 0);
            assert!(doc.is_empty().unwrap());
        }
    }

    #[test]
    fn test_multi_yaml_updater_remove_empty() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path)
                .unwrap()
                .remove_empty(true);
            let doc = updater.open().unwrap();

            // Remove all documents
            doc.remove(0).unwrap();

            updater.close().unwrap();
        }

        // File should be deleted
        assert!(!yaml_path.exists());
    }

    #[test]
    fn test_multi_yaml_updater_set_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let result = Python::attach(|py| {
                let new_dict = pyo3::types::PyDict::new(py);
                new_dict.set_item("doc", 999).unwrap();
                doc.set(999, new_dict.into())
            });

            assert!(result.is_err());
            match result {
                Err(Error::ValueError(msg)) => {
                    assert!(msg.contains("out of bounds"));
                }
                _ => panic!("Expected ValueError"),
            }
        }
    }

    #[test]
    fn test_multi_yaml_updater_remove_out_of_bounds() {
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("multi.yaml");

        fs::write(&yaml_path, "---\ndoc: 1\n").unwrap();

        {
            let mut updater = MultiYamlUpdater::new(&yaml_path).unwrap();
            let doc = updater.open().unwrap();

            let result = doc.remove(999);

            assert!(result.is_err());
            match result {
                Err(Error::ValueError(msg)) => {
                    assert!(msg.contains("out of bounds"));
                }
                _ => panic!("Expected ValueError"),
            }
        }
    }
}
