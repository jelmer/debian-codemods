use crate::{declare_fixer, FixerError, FixerResult};
use std::fs;
use std::path::Path;

const KEY_BLOCK_START: &[u8] = b"-----BEGIN PGP PUBLIC KEY BLOCK-----";
const KEY_BLOCK_END: &[u8] = b"-----END PGP PUBLIC KEY BLOCK-----";

/// Result of minimizing a key block
#[derive(Debug)]
enum MinimizeResult {
    /// No changes needed - key is already minimal
    NoChanges,
    /// Third-party signatures were removed
    SignaturesRemoved(Vec<u8>),
    /// Only format was upgraded (no signatures removed)
    FormatUpgraded(Vec<u8>),
}

/// Minimize a PGP key block by removing extra signatures
/// This keeps only self-signatures and removes third-party certifications
///
/// If opinionated is true, may upgrade packet format from old to new
/// If opinionated is false, preserves original format
fn minimize_key_block(
    key: &[u8],
    opinionated: bool,
) -> Result<MinimizeResult, Box<dyn std::error::Error>> {
    use sequoia_openpgp::cert::CertParser;
    use sequoia_openpgp::packet::Packet;
    use sequoia_openpgp::parse::{PacketParser, PacketParserResult, Parse};
    use sequoia_openpgp::serialize::Serialize;
    use sequoia_openpgp::KeyHandle;

    // First, use CertParser to understand the certificate structure
    // This tells us which keys belong to the cert so we can identify self-signatures
    let certs: Vec<_> = CertParser::from_bytes(key)?.collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err("No certificates found in key block".into());
    }

    // Collect KeyHandles for ALL certs (primary keys and all subkeys)
    let mut key_handles: Vec<KeyHandle> = Vec::new();
    for cert in &certs {
        key_handles.push(cert.fingerprint().into());
        key_handles.push(cert.keyid().into());
        for key in cert.keys() {
            key_handles.push(key.key().fingerprint().into());
            key_handles.push(key.key().keyid().into());
        }
    }

    // Now parse the ORIGINAL key data with PacketParser to preserve packet format
    // We'll filter out third-party signatures but keep everything else as-is
    let mut filtered_packets: Vec<Packet> = Vec::new();
    let mut ppr = PacketParser::from_bytes(key)?;
    let mut third_party_count = 0;

    while let PacketParserResult::Some(pp) = ppr {
        let (packet, next_ppr) = pp.recurse()?;

        match &packet {
            Packet::Signature(sig) => {
                // Check if this signature is from one of our keys
                let issuers = sig.get_issuers();
                let is_self_sig = issuers.iter().any(|issuer| key_handles.contains(issuer));

                if is_self_sig {
                    // Keep self-signatures
                    filtered_packets.push(packet);
                } else {
                    // Count but don't keep third-party signatures
                    third_party_count += 1;
                }
            }
            _ => {
                // Keep all non-signature packets (keys, user IDs, etc.) as-is
                filtered_packets.push(packet);
            }
        }

        ppr = next_ppr;
    }

    // Serialize the filtered packets
    use sequoia_openpgp::armor::{Kind, Writer};
    let is_armored = key.windows(5).any(|w| w == b"-----");
    let mut output = Vec::new();

    if is_armored {
        let mut writer = Writer::new(&mut output, Kind::PublicKey)?;
        for packet in &filtered_packets {
            Serialize::serialize(packet, &mut writer)?;
        }
        writer.finalize()?;
    } else {
        for packet in &filtered_packets {
            Serialize::serialize(packet, &mut output)?;
        }
    }

    // Determine what actually changed
    if third_party_count == 0 {
        // No signatures were removed
        if output == key {
            // Serialization is identical - no changes at all
            return Ok(MinimizeResult::NoChanges);
        } else if opinionated {
            // Serialization differs (format change) and opinionated mode allows it
            return Ok(MinimizeResult::FormatUpgraded(output));
        } else {
            // Serialization differs but we're not opinionated - don't upgrade format
            return Ok(MinimizeResult::NoChanges);
        }
    }

    // Signatures were removed
    Ok(MinimizeResult::SignaturesRemoved(output))
}

pub fn run(base_path: &Path, opinionated: bool) -> Result<FixerResult, FixerError> {
    let paths = [
        "debian/upstream/signing-key.asc",
        "debian/upstream/signing-key.pgp",
        "debian/upstream-signing-key.pgp",
    ];

    let mut signatures_removed = false;
    let mut format_upgraded = false;
    let mut tags_fixed = Vec::new();

    for path_str in &paths {
        let path = base_path.join(path_str);
        if !path.exists() {
            continue;
        }

        // Read the file
        let contents = fs::read(&path)?;
        let mut outlines: Vec<u8> = Vec::new();
        let mut key_block: Vec<u8> = Vec::new();
        let mut in_key_block = false;
        let mut i = 0;

        while i < contents.len() {
            // Find line boundaries
            let line_start = i;
            let line_end = contents[i..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|pos| i + pos + 1)
                .unwrap_or(contents.len());

            let line = &contents[line_start..line_end];
            let trimmed = line
                .iter()
                .filter(|&&b| b != b'\r' && b != b'\n')
                .copied()
                .collect::<Vec<u8>>();

            if trimmed == KEY_BLOCK_START {
                in_key_block = true;
                key_block.clear();
                key_block.extend_from_slice(line);
            } else if trimmed == KEY_BLOCK_END && in_key_block {
                key_block.extend_from_slice(line);

                // Process the key block
                match minimize_key_block(&key_block, opinionated) {
                    Ok(MinimizeResult::NoChanges) => {
                        // Keep original key block
                        outlines.extend_from_slice(&key_block);
                    }
                    Ok(MinimizeResult::SignaturesRemoved(minimized)) => {
                        outlines.extend_from_slice(&minimized);
                        signatures_removed = true;
                        tags_fixed.push("public-upstream-key-not-minimal");
                    }
                    Ok(MinimizeResult::FormatUpgraded(upgraded)) => {
                        outlines.extend_from_slice(&upgraded);
                        format_upgraded = true;
                    }
                    Err(e) => {
                        return Err(FixerError::Other(format!("Failed to minimize key: {}", e)));
                    }
                }

                in_key_block = false;
                key_block.clear();
            } else if in_key_block {
                key_block.extend_from_slice(line);
            } else {
                outlines.extend_from_slice(line);
            }

            i = line_end;
        }

        if in_key_block {
            return Err(FixerError::Other("Key block without end".to_string()));
        }

        // Check if contents changed
        if contents != outlines {
            fs::write(&path, &outlines)?;
        }
    }

    // Return appropriate result based on what changed
    if signatures_removed {
        Ok(
            FixerResult::builder("Re-export upstream signing key without extra signatures.")
                .fixed_tags(tags_fixed)
                .build(),
        )
    } else if format_upgraded {
        Ok(FixerResult::builder("Upgrade upstream signing key to new packet format.").build())
    } else {
        Err(FixerError::NoChanges)
    }
}

declare_fixer! {
    name: "public-upstream-key-not-minimal",
    tags: ["public-upstream-key-not-minimal"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences.opinionated.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_minimize_key() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        // Use the actual test fixture key
        let test_fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "tests/public-upstream-key-not-minimal/simple/in/debian/upstream/signing-key.asc",
        );

        // Skip test if fixture doesn't exist
        if !test_fixture_path.exists() {
            eprintln!(
                "Skipping test: fixture not found at {:?}",
                test_fixture_path
            );
            return;
        }

        let input_key = fs::read(&test_fixture_path).unwrap();
        let key_path = upstream_dir.join("signing-key.asc");
        fs::write(&key_path, &input_key).unwrap();

        // Apply the fixer (not opinionated)
        let result = run(temp_dir.path(), false);
        assert!(result.is_ok());

        // Check that the file was modified and is smaller
        let output_key = fs::read(&key_path).unwrap();
        assert!(output_key.len() < input_key.len());

        // Verify the key is still valid (may be a keyring with multiple certs)
        use sequoia_openpgp::cert::CertParser;
        use sequoia_openpgp::parse::Parse;
        let certs: Vec<_> = CertParser::from_bytes(&output_key)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            !certs.is_empty(),
            "Output should contain at least one valid cert"
        );
    }

    #[test]
    fn test_already_minimal() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        // Use the already-minimal test fixture
        let test_fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-upstream-key-not-minimal/already-minimal/in/debian/upstream/signing-key.asc");

        if !test_fixture_path.exists() {
            eprintln!(
                "Skipping test: fixture not found at {:?}",
                test_fixture_path
            );
            return;
        }

        let input_key = fs::read(&test_fixture_path).unwrap();
        let key_path = upstream_dir.join("signing-key.asc");
        fs::write(&key_path, &input_key).unwrap();

        // Apply the fixer (not opinionated)
        let result = run(temp_dir.path(), false);

        // Should return NoChanges if already minimal
        assert!(matches!(result, Err(FixerError::NoChanges)));

        // Verify file wasn't changed
        let output_key = fs::read(&key_path).unwrap();
        assert_eq!(
            input_key, output_key,
            "File should not be modified when already minimal"
        );
    }

    #[test]
    fn test_already_minimal_opinionated() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let upstream_dir = debian_dir.join("upstream");
        fs::create_dir_all(&upstream_dir).unwrap();

        // Use the already-minimal test fixture (in old GPG format)
        let test_fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-upstream-key-not-minimal/already-minimal/in/debian/upstream/signing-key.asc");

        if !test_fixture_path.exists() {
            eprintln!(
                "Skipping test: fixture not found at {:?}",
                test_fixture_path
            );
            return;
        }

        let input_key = fs::read(&test_fixture_path).unwrap();
        let key_path = upstream_dir.join("signing-key.asc");
        fs::write(&key_path, &input_key).unwrap();

        // Apply the fixer with opinionated=true
        let result = run(temp_dir.path(), true);
        assert!(
            result.is_ok(),
            "Opinionated mode should upgrade format: {:?}",
            result
        );

        let result = result.unwrap();
        assert_eq!(
            result.description, "Upgrade upstream signing key to new packet format.",
            "Should report format upgrade, not tag fix"
        );
        assert!(
            result.fixed_lintian_tags().is_empty(),
            "Should not report any lintian tags as fixed when only upgrading format"
        );

        // Verify file was changed (format upgraded)
        let output_key = fs::read(&key_path).unwrap();
        assert_ne!(
            input_key, output_key,
            "File should be modified in opinionated mode"
        );
    }

    #[test]
    fn test_no_key_file() {
        let temp_dir = TempDir::new().unwrap();

        // Apply the fixer
        let result = run(temp_dir.path(), false);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
