use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, FilesystemAction};
use crate::workspace::FixerWorkspace;
use crate::{Certainty, FixerError, FixerPreferences, LintianIssue};
use sequoia_openpgp::armor::{Kind, Writer};
use sequoia_openpgp::cert::Cert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::serialize::Serialize;
use std::path::PathBuf;

const MAIN: &str = "debian/upstream/signing-key.asc";
const OTHERS: &[&str] = &[
    "debian/upstream/signing-key.pgp",
    "debian/upstream-signing-key.pgp",
];

fn merge_keys(key_data: Vec<Vec<u8>>) -> Result<String, Box<dyn std::error::Error>> {
    let mut all_certs = Vec::new();
    for data in key_data {
        match Cert::from_bytes(&data) {
            Ok(cert) => all_certs.push(cert),
            Err(_) => {
                use sequoia_openpgp::cert::CertParser;
                let parser = CertParser::from_bytes(&data)?;
                for cert_result in parser {
                    match cert_result {
                        Ok(cert) => all_certs.push(cert),
                        Err(e) => {
                            tracing::debug!("failed to parse one certificate: {}", e);
                        }
                    }
                }
            }
        }
    }
    if all_certs.is_empty() {
        return Err("No valid certificates found in any of the key files".into());
    }

    let mut output = Vec::new();
    {
        let mut writer = Writer::new(&mut output, Kind::PublicKey)?;
        for cert in all_certs {
            cert.serialize(&mut writer)?;
        }
        writer.finalize()?;
    }
    Ok(String::from_utf8(output)?)
}

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // Walk other locations first then the main location, matching the
    // legacy ordering (this affects the LintianIssue's info list).
    let mut existing: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for rel in OTHERS {
        let p = PathBuf::from(rel);
        if let Some(data) = ws.read_file(&p)? {
            existing.push((p, data));
        }
    }
    let main_rel = PathBuf::from(MAIN);
    if let Some(data) = ws.read_file(&main_rel)? {
        existing.push((main_rel.clone(), data));
    }

    if existing.len() < 2 {
        return Ok(Vec::new());
    }

    let key_data: Vec<Vec<u8>> = existing.iter().map(|(_, d)| d.clone()).collect();
    let merged = merge_keys(key_data)
        .map_err(|e| FixerError::Other(format!("Failed to merge keys: {}", e)))?;

    let info: Vec<String> = existing
        .iter()
        .map(|(p, _)| p.display().to_string())
        .collect();
    let issue = LintianIssue::source_with_info("public-upstream-keys-in-multiple-locations", info);

    let mut actions: Vec<Action> = vec![Action::Filesystem(FilesystemAction::Write {
        file: main_rel,
        content: merged.into_bytes(),
    })];
    for rel in OTHERS {
        let p = PathBuf::from(rel);
        if ws.read_file(&p)?.is_some() {
            actions.push(Action::Filesystem(FilesystemAction::Delete { file: p }));
        }
    }

    Ok(vec![Diagnostic::with_actions(
        issue,
        "Merge upstream signing key files.",
        actions,
    )
    .with_certainty(Certainty::Certain)])
}

declare_detector! {
    name: "public-upstream-keys-in-multiple-locations",
    tags: ["public-upstream-keys-in-multiple-locations"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
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
    fn test_single_key_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        fs::write(debian.join("upstream-signing-key.pgp"), b"dummy").unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_key_files() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir(&debian).unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_debian_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
