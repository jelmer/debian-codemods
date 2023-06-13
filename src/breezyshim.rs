// TODO(jelmer): Use breezy::RevisionId instead
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RevisionId(Vec<u8>);

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
