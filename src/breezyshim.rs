// TODO(jelmer): Use breezy::RevisionId instead
use pyo3::prelude::*;
use std::io::Read;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RevisionId(Vec<u8>);

use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl RevisionId {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for RevisionId {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl Serialize for RevisionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(String::from_utf8(self.0.clone()).unwrap().as_str())
    }
}

impl<'de> Deserialize<'de> for RevisionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|s| Self(s.into_bytes()))
    }
}

impl FromPyObject<'_> for RevisionId {
    fn extract(ob: &'_ PyAny) -> PyResult<Self> {
        let bytes = ob.extract::<Vec<u8>>()?;
        Ok(Self(bytes))
    }
}

impl ToPyObject for RevisionId {
    fn to_object(&self, py: Python) -> PyObject {
        pyo3::types::PyBytes::new(py, &self.0).to_object(py)
    }
}

pub struct Lock(PyObject);

impl Drop for Lock {
    fn drop(&mut self) {
        Python::with_gil(|py| {
            self.0.call_method0(py, "unlock").unwrap();
        });
    }
}

pub trait Tree {
    fn obj(&self) -> &PyObject;

    fn get_file(&self, path: &std::path::Path) -> PyResult<Box<dyn std::io::Read>> {
        Python::with_gil(|py| {
            let f = self.obj().call_method1(py, "get_file", (path,))?;

            let f = pyo3_file::PyFileLikeObject::with_requirements(f, true, false, false)?;

            Ok(Box::new(f) as Box<dyn std::io::Read>)
        })
    }

    fn lock_read(&self) -> PyResult<Lock> {
        Python::with_gil(|py| {
            let lock = self.obj().call_method0(py, "lock_read").unwrap();
            Ok(Lock(lock))
        })
    }

    fn has_filename(&self, path: &std::path::Path) -> bool {
        Python::with_gil(|py| {
            self.obj()
                .call_method1(py, "has_filename", (path,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn get_parent_ids(&self) -> PyResult<Vec<RevisionId>> {
        Python::with_gil(|py| {
            self.obj()
                .call_method0(py, "get_parent_ids")
                .unwrap()
                .extract(py)
        })
    }

    fn is_ignored(&self, path: &std::path::Path) -> Option<String> {
        Python::with_gil(|py| {
            self.obj()
                .call_method1(py, "is_ignored", (path,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn is_versioned(&self, path: &std::path::Path) -> bool {
        Python::with_gil(|py| {
            self.obj()
                .call_method1(py, "is_versioned", (path,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn iter_changes(
        &self,
        other: &Box<dyn Tree>,
        specific_files: Option<&[&std::path::Path]>,
        want_unversioned: Option<bool>,
        require_versioned: Option<bool>,
    ) -> PyResult<Box<dyn Iterator<Item = PyResult<TreeChange>>>> {
        Python::with_gil(|py| {
            let kwargs = pyo3::types::PyDict::new(py);
            if let Some(specific_files) = specific_files {
                kwargs.set_item("specific_files", specific_files)?;
            }
            if let Some(want_unversioned) = want_unversioned {
                kwargs.set_item("want_unversioned", want_unversioned)?;
            }
            if let Some(require_versioned) = require_versioned {
                kwargs.set_item("require_versioned", require_versioned)?;
            }
            struct TreeChangeIter(pyo3::PyObject);

            impl Iterator for TreeChangeIter {
                type Item = PyResult<TreeChange>;

                fn next(&mut self) -> Option<Self::Item> {
                    Python::with_gil(|py| {
                        let next = match self.0.call_method0(py, "__next__") {
                            Ok(v) => v,
                            Err(e) => {
                                if e.is_instance_of::<pyo3::exceptions::PyStopIteration>(py) {
                                    return None;
                                }
                                return Some(Err(e));
                            }
                        };
                        if next.is_none(py) {
                            None
                        } else {
                            Some(next.extract(py))
                        }
                    })
                }
            }

            Ok(Box::new(TreeChangeIter(self.obj().call_method(
                py,
                "iter_changes",
                (other.obj(),),
                Some(kwargs),
            )?))
                as Box<dyn Iterator<Item = PyResult<TreeChange>>>)
        })
    }

    fn has_versioned_directories(&self) -> bool {
        Python::with_gil(|py| {
            self.obj()
                .call_method0(py, "has_versioned_directories")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }
}

pub struct RevisionTree(pub PyObject);

impl Tree for RevisionTree {
    fn obj(&self) -> &PyObject {
        &self.0
    }
}

pub struct WorkingTree(pub PyObject);

impl WorkingTree {
    pub fn basis_tree(&self) -> Box<dyn Tree> {
        Python::with_gil(|py| {
            let tree = self.0.call_method0(py, "basis_tree").unwrap();
            Box::new(RevisionTree(tree))
        })
    }

    pub fn abspath(&self, path: &std::path::Path) -> std::path::PathBuf {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "abspath", (path,))
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    pub fn supports_setting_file_ids(&self) -> bool {
        Python::with_gil(|py| {
            self.0
                .call_method0(py, "supports_setting_file_ids")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    pub fn add(&self, paths: &[&std::path::Path]) -> PyResult<()> {
        Python::with_gil(|py| {
            self.0.call_method1(py, "add", (paths.to_vec(),)).unwrap();
        });
        Ok(())
    }

    pub fn smart_add(&self, paths: &[&std::path::Path]) -> PyResult<()> {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "smart_add", (paths.to_vec(),))
                .unwrap();
        });
        Ok(())
    }

    pub fn commit(
        &self,
        message: &str,
        allow_pointless: Option<bool>,
        committer: Option<&str>,
        specific_files: Option<&[&std::path::Path]>,
    ) -> PyResult<RevisionId> {
        Python::with_gil(|py| {
            let kwargs = pyo3::types::PyDict::new(py);
            if let Some(committer) = committer {
                kwargs.set_item("committer", committer).unwrap();
            }
            if let Some(specific_files) = specific_files {
                kwargs.set_item("specific_files", specific_files).unwrap();
            }
            if let Some(allow_pointless) = allow_pointless {
                kwargs.set_item("allow_pointless", allow_pointless).unwrap();
            }

            let null_commit_reporter = py
                .import("breezy.commit")?
                .getattr("NullCommitReporter")?
                .call0()?;
            kwargs.set_item("reporter", null_commit_reporter).unwrap();

            self.0
                .call_method(py, "commit", (message,), Some(kwargs))
                .unwrap()
                .extract(py)
        })
    }
}

impl Tree for WorkingTree {
    fn obj(&self) -> &PyObject {
        &self.0
    }
}

pub struct DirtyTracker(pub PyObject);

impl DirtyTracker {
    pub fn is_dirty(&self) -> bool {
        Python::with_gil(|py| {
            self.0
                .call_method0(py, "is_dirty")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    pub fn relpaths(&self) -> impl IntoIterator<Item = std::path::PathBuf> {
        Python::with_gil(|py| {
            let set = self
                .0
                .call_method0(py, "relpaths")
                .unwrap()
                .extract::<std::collections::HashSet<_>>(py)
                .unwrap();
            set.into_iter()
        })
    }
}

pub fn reset_tree(
    local_tree: &WorkingTree,
    basis_tree: Option<&Box<dyn Tree>>,
    subpath: Option<&std::path::Path>,
    dirty_tracker: Option<&DirtyTracker>,
) -> PyResult<()> {
    Python::with_gil(|py| {
        let workspace_m = py.import("breezy.workspace")?;
        let reset_tree = workspace_m.getattr("reset_tree")?;
        let local_tree: PyObject = local_tree.obj().clone_ref(py);
        let basis_tree: Option<PyObject> = basis_tree.map(|o| o.obj().clone_ref(py));
        let dirty_tracker: Option<PyObject> = dirty_tracker.map(|dt| dt.0.clone());
        reset_tree.call1((local_tree, basis_tree, subpath, dirty_tracker))?;
        Ok(())
    })
}

#[derive(Debug)]
pub struct TreeChange {
    pub path: (Option<std::path::PathBuf>, Option<std::path::PathBuf>),
    pub changed_content: bool,
    pub versioned: (Option<bool>, Option<bool>),
    pub name: (Option<std::ffi::OsString>, Option<std::ffi::OsString>),
    pub kind: (Option<String>, Option<String>),
    pub executable: (Option<bool>, Option<bool>),
    pub copied: bool,
}

impl ToPyObject for TreeChange {
    fn to_object(&self, py: Python) -> PyObject {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("path", &self.path).unwrap();
        dict.set_item("changed_content", self.changed_content)
            .unwrap();
        dict.set_item("versioned", self.versioned).unwrap();
        dict.set_item("name", &self.name).unwrap();
        dict.set_item("kind", &self.kind).unwrap();
        dict.set_item("executable", self.executable).unwrap();
        dict.set_item("copied", self.copied).unwrap();
        let m = py.import("breezy.tree").unwrap();
        m.getattr("TreeChange")
            .unwrap()
            .call1((dict,))
            .unwrap()
            .into()
    }
}

impl FromPyObject<'_> for TreeChange {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        fn from_bool(o: &PyAny) -> PyResult<bool> {
            if let Ok(b) = o.extract::<isize>() {
                Ok(b != 0)
            } else {
                o.extract::<bool>()
            }
        }

        fn from_opt_bool_tuple(o: &PyAny) -> PyResult<(Option<bool>, Option<bool>)> {
            let tuple = o.extract::<(Option<&PyAny>, Option<&PyAny>)>()?;
            Ok((
                tuple.0.map(from_bool).transpose()?,
                tuple.1.map(from_bool).transpose()?,
            ))
        }

        let path = obj.getattr("path")?;
        let changed_content = from_bool(obj.getattr("changed_content")?)?;

        let versioned = from_opt_bool_tuple(obj.getattr("versioned")?)?;
        let name = obj.getattr("name")?;
        let kind = obj.getattr("kind")?;
        let executable = from_opt_bool_tuple(obj.getattr("executable")?)?;
        let copied = obj.getattr("copied")?;

        Ok(TreeChange {
            path: path.extract()?,
            changed_content,
            versioned,
            name: name.extract()?,
            kind: kind.extract()?,
            executable,
            copied: copied.extract()?,
        })
    }
}
