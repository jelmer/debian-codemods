use crate::relations::ensure_relation;
use crate::relations::is_relation_implied;
use debian_control::lossless::relations::{Entry, Relations};

/// Interface for editing debian packages, whether backed by real control files or debcargo files.
pub trait AbstractControlEditor {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource + 'a>>;

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>>;

    fn commit(&self) -> bool;
}

pub trait AbstractSource<'a> {
    fn name(&self) -> Option<String>;

    fn ensure_build_dep(&mut self, dep: Entry);
}

pub trait AbstractBinary {
    fn name(&self) -> Option<String>;
}

use crate::debcargo::{DebcargoBinary, DebcargoEditor, DebcargoSource};
use debian_control::{Binary as PlainBinary, Control as PlainControl, Source as PlainSource};

impl AbstractControlEditor for DebcargoEditor {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource<'a> + 'a>> {
        Some(Box::new(DebcargoEditor::source(self)) as Box<dyn AbstractSource>)
    }

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>> {
        DebcargoEditor::binaries(self)
            .map(|b| Box::new(b) as Box<dyn AbstractBinary>)
            .collect()
    }

    fn commit(&self) -> bool {
        DebcargoEditor::commit(self).unwrap()
    }
}

impl AbstractBinary for PlainBinary {
    fn name(&self) -> Option<String> {
        self.name()
    }
}

impl<'a> AbstractSource<'a> for PlainSource {
    fn name(&self) -> Option<String> {
        self.name()
    }

    fn ensure_build_dep(&mut self, dep: Entry) {
        if let Some(mut build_deps) = self.build_depends() {
            ensure_relation(&mut build_deps, dep);
        } else {
            self.set_build_depends(&Relations::from(vec![dep]));
        }
    }
}

impl<'a> AbstractBinary for DebcargoBinary<'a> {
    fn name(&self) -> Option<String> {
        Some(self.name().to_string())
    }
}

impl<'a> AbstractSource<'a> for DebcargoSource<'a> {
    fn name(&self) -> Option<String> {
        self.name()
    }

    fn ensure_build_dep(&mut self, dep: Entry) {
        // TODO: Check that it's not already there
        if let Some(build_deps) = self
            .toml_section_mut()
            .get_mut("build_depends")
            .and_then(|v| v.as_array_mut())
        {
            build_deps.push(dep.to_string());
        }
    }
}

impl<E: crate::editor::Editor<PlainControl>> AbstractControlEditor for E {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource + 'a>> {
        PlainControl::source(self).map(|s| Box::new(s) as Box<dyn AbstractSource>)
    }

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>> {
        PlainControl::binaries(self)
            .map(|b| Box::new(b) as Box<dyn AbstractBinary>)
            .collect()
    }

    fn commit(&self) -> bool {
        !(self as &dyn crate::editor::Editor<PlainControl>)
            .commit()
            .unwrap()
            .is_empty()
    }
}
