use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences};
use std::path::PathBuf;

fn convert_key_to_armor(binary_key: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    use sequoia_openpgp::armor::{Kind, Writer};
    use sequoia_openpgp::cert::Cert;
    use sequoia_openpgp::parse::Parse;
    use sequoia_openpgp::serialize::Serialize;

    let cert = Cert::from_bytes(binary_key)?;
    let mut armored = Vec::new();
    {
        let mut writer = Writer::new(&mut armored, Kind::PublicKey)?;
        cert.serialize(&mut writer)?;
        writer.finalize()?;
    }
    Ok(String::from_utf8(armored)?)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    let pgp_rel = PathBuf::from("debian/upstream/signing-key.pgp");
    let asc_rel = PathBuf::from("debian/upstream/signing-key.asc");
    let binary_key = match ws.read_file(&pgp_rel)? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    let armored = convert_key_to_armor(&binary_key)
        .map_err(|e| FixerError::Other(format!("Failed to convert key to armor: {}", e)))?;

    Ok(vec![Diagnostic::untagged(
        "Enarmor upstream signing key.",
        vec![
            Action::Filesystem(FilesystemAction::Write {
                file: asc_rel,
                content: armored.into_bytes(),
            }),
            Action::Filesystem(FilesystemAction::Delete { file: pgp_rel }),
        ],
    )])
}

declare_detector! {
    name: "public-upstream-key-binary",
    tags: [],
    triggers: [
        crate::workspace::Trigger::File("debian/upstream/signing-key.pgp"),
    ],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let version: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &version, &FixerPreferences::default())
    }

    #[test]
    fn test_convert_binary_key_to_armored() {
        let tmp = TempDir::new().unwrap();
        let upstream = tmp.path().join("debian/upstream");
        fs::create_dir_all(&upstream).unwrap();

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-upstream-key-binary/simple/in/debian/upstream/signing-key.pgp");
        if !fixture.exists() {
            eprintln!("Skipping test: fixture not found at {:?}", fixture);
            return;
        }
        let pgp = upstream.join("signing-key.pgp");
        fs::write(&pgp, fs::read(&fixture).unwrap()).unwrap();

        run_apply(tmp.path()).unwrap();
        let asc = upstream.join("signing-key.asc");
        assert!(asc.exists());
        assert!(!pgp.exists());

        let content = fs::read_to_string(&asc).unwrap();
        assert!(content.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
        use sequoia_openpgp::cert::Cert;
        use sequoia_openpgp::parse::Parse;
        Cert::from_bytes(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_no_binary_key_file() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
