/// Interface for editing debian packages, whether backed by real control files or debcargo files.
pub trait AbstractControl {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource + 'a>>;

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>>;
}

pub trait AbstractSource<'a> {
    fn name(&self) -> Option<String>;
}

pub trait AbstractBinary {
    fn name(&self) -> Option<String>;
}

use crate::debcargo::{DebcargoBinary, DebcargoEditor as DebcargoControl, DebcargoSource};
use debian_control::{Binary as PlainBinary, Control as PlainControl, Source as PlainSource};

impl AbstractControl for PlainControl {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource + 'a>> {
        PlainControl::source(self).map(|s| Box::new(s) as Box<dyn AbstractSource>)
    }

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>> {
        PlainControl::binaries(self)
            .map(|b| Box::new(b) as Box<dyn AbstractBinary>)
            .collect()
    }
}

impl AbstractControl for DebcargoControl {
    fn source<'a>(&'a mut self) -> Option<Box<dyn AbstractSource<'a> + 'a>> {
        Some(Box::new(DebcargoControl::source(self)) as Box<dyn AbstractSource>)
    }

    fn binaries<'a>(&'a mut self) -> Vec<Box<dyn AbstractBinary + 'a>> {
        DebcargoControl::binaries(self)
            .map(|b| Box::new(b) as Box<dyn AbstractBinary>)
            .collect()
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
}
