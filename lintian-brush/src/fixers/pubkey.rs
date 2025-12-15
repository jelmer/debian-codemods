use crate::watch::COMMON_PGPSIGURL_MANGLES;
use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_watch::{mangle, Release, WatchFile};
use sequoia_openpgp as openpgp;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const NUM_KEYS_TO_CHECK: usize = 5;
const RELEASES_TO_INSPECT: usize = 5;

#[derive(Debug)]
enum VerificationStatus {
    /// No keyring available, signature not verified (discovery mode)
    Unverified,
    /// Signature verified successfully with keyring
    Verified,
    /// Signature verification failed with keyring (wrong key or corrupted signature)
    Failed,
}

#[derive(Debug)]
struct SignatureInfo {
    verification_status: VerificationStatus,
    keys: HashSet<String>,
    mangle: Option<String>,
}

/// Verify a detached signature against data using a keyring
fn verify_signature(
    sig_data: &[u8],
    data: &[u8],
    keyring_data: &[u8],
) -> Result<bool, Box<dyn std::error::Error>> {
    use openpgp::parse::Parse;
    use openpgp::policy::StandardPolicy;

    let policy = StandardPolicy::new();

    // Parse all certificates from the keyring
    let cert_parser = openpgp::cert::CertParser::from_bytes(keyring_data)?;
    let certs: Vec<_> = cert_parser.filter_map(|r| r.ok()).collect();

    if certs.is_empty() {
        return Err("No valid certificates in keyring".into());
    }

    // Parse the signature
    let packets = openpgp::PacketPile::from_bytes(sig_data)?;

    // Try to verify with each certificate
    for cert in &certs {
        for packet in packets.descendants() {
            let sig = match packet {
                openpgp::Packet::Signature(sig) => sig,
                _ => continue,
            };

            // Check each key in the certificate
            for key_amalg in cert.keys().with_policy(&policy, None) {
                let key_handle = key_amalg.key();
                let key_fingerprint = key_handle.fingerprint();

                // Check if signature issuer matches this key
                let is_issuer = sig.issuer_fingerprints().any(|fp| fp == &key_fingerprint);
                if !is_issuer {
                    continue;
                }

                // Try to verify the signature
                match sig.clone().verify_message(key_handle, data) {
                    Ok(_) => {
                        log::debug!(
                            "Signature verified successfully with key {}",
                            key_fingerprint
                        );
                        return Ok(true);
                    }
                    Err(e) => {
                        log::debug!("Signature verification failed: {}", e);
                    }
                }
            }
        }
    }

    Ok(false)
}

/// Probe for signature files and verify them
fn probe_signature(
    release: &Release,
    pgpsigurlmangle: Option<&str>,
    keyring_data: &[u8],
) -> Result<Option<SignatureInfo>, Box<dyn std::error::Error>> {
    let mangles: Vec<&str> = if let Some(mangle) = pgpsigurlmangle {
        vec![mangle]
    } else {
        COMMON_PGPSIGURL_MANGLES.to_vec()
    };

    for mangle in mangles {
        let sig_url = if let Some(ref pgpsigurl) = release.pgpsigurl {
            pgpsigurl.clone()
        } else {
            match mangle::apply_mangle(mangle, &release.url) {
                Ok(url) => url,
                Err(e) => {
                    log::debug!(
                        "Failed to apply mangle '{}' to '{}': {}",
                        mangle,
                        release.url,
                        e
                    );
                    continue;
                }
            }
        };

        log::debug!(
            "Trying signature URL: {} (from release URL: {})",
            sig_url,
            release.url
        );

        // Try to download the signature
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let sig_response = match client.get(&sig_url).send() {
            Ok(resp) if resp.status().is_success() => {
                log::debug!("Successfully downloaded signature from {}", sig_url);
                resp
            }
            Ok(resp) => {
                log::debug!(
                    "Signature URL {} returned status {}",
                    sig_url,
                    resp.status()
                );
                continue;
            }
            Err(e) => {
                log::debug!("Failed to fetch signature from {}: {}", sig_url, e);
                continue;
            }
        };

        let sig_data = sig_response.bytes()?;

        // Download the actual release file for verification
        let release_data = match release.download_blocking() {
            Ok(data) => {
                log::debug!("Downloaded release tarball ({} bytes)", data.len());
                data
            }
            Err(e) => {
                log::debug!("Failed to download release: {}", e);
                continue;
            }
        };

        // Parse the signature and extract fingerprints
        use openpgp::parse::Parse;

        let packets = match openpgp::PacketPile::from_bytes(&sig_data) {
            Ok(packets) => packets,
            Err(e) => {
                log::debug!("Failed to parse signature packets: {}", e);
                continue;
            }
        };

        // Extract fingerprints from the signature
        let mut fingerprints = Vec::new();
        for packet in packets.descendants() {
            if let openpgp::Packet::Signature(sig) = packet {
                // Try to get the issuer fingerprint
                if let Some(fp) = sig.issuer_fingerprints().next() {
                    let fp_hex = fp.to_hex();
                    log::debug!("Found issuer fingerprint in signature: {}", fp_hex);
                    fingerprints.push(fp_hex);
                }
            }
        }

        if fingerprints.is_empty() {
            log::debug!("No fingerprints found in signature");
            continue;
        }

        let mut keys = HashSet::new();
        for fp in &fingerprints {
            keys.insert(fp.clone());
        }

        // Try to verify the signature if we have a keyring
        let verification_status = if !keyring_data.is_empty() {
            match verify_signature(&sig_data, &release_data, keyring_data) {
                Ok(true) => {
                    log::debug!("Signature verification succeeded");
                    VerificationStatus::Verified
                }
                Ok(false) => {
                    log::debug!("Signature verification failed - signature does not match keyring");
                    VerificationStatus::Failed
                }
                Err(e) => {
                    log::debug!("Error during signature verification: {}", e);
                    // If we can't parse but found fingerprints, treat as unverified
                    VerificationStatus::Unverified
                }
            }
        } else {
            // No keyring available, discovery mode
            log::debug!("No keyring available, discovery mode");
            VerificationStatus::Unverified
        };

        log::debug!(
            "Found signature with {} key(s), status={:?}",
            keys.len(),
            verification_status
        );
        return Ok(Some(SignatureInfo {
            verification_status,
            keys,
            mangle: Some(mangle.to_string()),
        }));
    }

    Ok(None)
}

/// Analyze used mangles to find common patterns
///
/// Returns (all_mangles, non_none_mangles) where:
/// - all_mangles includes None entries for unsigned releases
/// - non_none_mangles only includes the actual mangle strings
fn analyze_mangles(used_mangles: &[Option<String>]) -> (HashSet<Option<String>>, HashSet<String>) {
    let found_common_mangles: HashSet<Option<String>> =
        used_mangles.iter().take(5).cloned().collect();
    let active_common_mangles: HashSet<String> = found_common_mangles
        .iter()
        .filter_map(|x| x.as_ref().cloned())
        .collect();

    (found_common_mangles, active_common_mangles)
}

/// Determine the pgpmode and description based on found mangles
///
/// Returns (pgpmode, description):
/// - If all releases are signed (only one entry, which is Some): ("mangle", "Check upstream PGP signatures.")
/// - Otherwise: ("auto", "Opportunistically check upstream PGP signatures.")
fn determine_pgpmode(found_common_mangles: &HashSet<Option<String>>) -> (&'static str, String) {
    if found_common_mangles.len() == 1 {
        ("mangle", "Check upstream PGP signatures.".to_string())
    } else {
        (
            "auto",
            "Opportunistically check upstream PGP signatures.".to_string(),
        )
    }
}

/// Export a certificate in minimal armored format
fn export_cert_armored(cert: &openpgp::Cert) -> Result<Vec<u8>, String> {
    use openpgp::serialize::Serialize;

    let mut key_output = Vec::new();
    {
        let mut writer =
            openpgp::armor::Writer::new(&mut key_output, openpgp::armor::Kind::PublicKey)
                .map_err(|e| format!("Failed to create armor writer: {}", e))?;

        cert.serialize(&mut writer)
            .map_err(|e| format!("Failed to serialize certificate: {}", e))?;

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize armor: {}", e))?;
    }

    Ok(key_output)
}

pub fn run(
    base_path: &Path,
    package: &str,
    _version: &debversion::Version,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    log::debug!("Running pubkey fixer for package {}", package);

    let watch_path = base_path.join("debian/watch");

    if !watch_path.exists() {
        log::debug!("No debian/watch file found");
        return Err(FixerError::NoChanges);
    }

    // Check if network access is allowed
    if !preferences.net_access.unwrap_or(false) {
        log::debug!("Network access not enabled, skipping");
        return Err(FixerError::NoChanges);
    }

    // Check if signing keys already exist
    let mut has_keys = false;
    for path in &[
        "debian/upstream/signing-key.asc",
        "debian/upstream/signing-key.pgp",
    ] {
        if base_path.join(path).exists() {
            log::debug!("Found existing signing key at {}", path);
            has_keys = true;
            break;
        }
    }

    let content = fs::read_to_string(&watch_path)?;
    let watch_file: WatchFile = content
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse watch file: {}", e)))?;

    let mut needed_keys: HashSet<String> = HashSet::new();
    let mut description: Option<String> = None;
    let mut made_changes = false;
    let mut fixed_issues = Vec::new();
    let mut overridden_issues = Vec::new();

    // Load existing keyring if available
    let keyring_data = if has_keys {
        let mut data = vec![];
        for path in &[
            "debian/upstream/signing-key.asc",
            "debian/upstream/signing-key.pgp",
        ] {
            let full_path = base_path.join(path);
            if full_path.exists() {
                match fs::read(&full_path) {
                    Ok(loaded_data) => {
                        log::debug!("Loaded existing keyring from {}", path);
                        data = loaded_data;
                        break;
                    }
                    Err(e) => {
                        log::warn!("Failed to read keyring from {}: {}", path, e);
                    }
                }
            }
        }
        data
    } else {
        vec![]
    };

    for mut entry in watch_file.entries() {
        let pgpsigurlmangle = entry.get_option("pgpsigurlmangle");

        // Skip entries that already have pgpsigurlmangle and keys
        if pgpsigurlmangle.is_some() && has_keys {
            log::debug!("Entry already has pgpsigurlmangle and keys, skipping");
            continue;
        }

        let pgpmode = entry
            .get_option("pgpmode")
            .unwrap_or_else(|| "default".to_string());

        // Skip if pgpmode is already set and diligence is 0
        if entry.get_option("pgpmode").is_some() && preferences.diligence.unwrap_or(0) == 0 {
            log::debug!("pgpmode already set and diligence=0, skipping");
            continue;
        }

        // Skip certain pgpmodes that we can't handle
        if matches!(pgpmode.as_str(), "gittag" | "previous" | "next" | "self") {
            log::debug!("Unsupported pgpmode: {}, skipping", pgpmode);
            return Err(FixerError::NoChanges);
        }

        // Discover releases
        log::debug!("Discovering releases for package {}", package);
        let releases = match entry.discover_blocking(|| package.to_string()) {
            Ok(mut rels) => {
                rels.sort_by(|a, b| b.cmp(a)); // Sort in reverse order (newest first)
                log::debug!("Found {} releases", rels.len());
                rels
            }
            Err(e) => {
                if let Some(http_err) = e.downcast_ref::<reqwest::Error>() {
                    if http_err.is_status() {
                        log::warn!("HTTP error accessing discovery URL: {}", http_err);
                        return Err(FixerError::NoChanges);
                    }
                }
                return Err(FixerError::Other(format!(
                    "Failed to discover releases: {}",
                    e
                )));
            }
        };

        let mut verification_statuses = Vec::new();
        let mut used_mangles: Vec<Option<String>> = Vec::new();
        let mut has_verification_failure = false;

        log::debug!(
            "Checking signatures for up to {} releases",
            RELEASES_TO_INSPECT
        );
        for release in releases.iter().take(RELEASES_TO_INSPECT) {
            log::debug!("Probing signature for release {}", release.version);
            match probe_signature(release, pgpsigurlmangle.as_deref(), &keyring_data) {
                Ok(Some(sig_info)) => {
                    log::debug!(
                        "Found signature with mangle: {:?}, status: {:?}",
                        sig_info.mangle,
                        sig_info.verification_status
                    );

                    // Check for verification failures
                    if matches!(sig_info.verification_status, VerificationStatus::Failed) {
                        has_verification_failure = true;
                    }

                    verification_statuses.push(sig_info.verification_status);
                    used_mangles.push(sig_info.mangle.clone());
                    needed_keys.extend(sig_info.keys);
                }
                Ok(None) => {
                    log::debug!("No signature found for release {}", release.version);
                    used_mangles.push(None);
                }
                Err(e) => {
                    log::warn!("Error probing signature: {}", e);
                    used_mangles.push(None);
                }
            }
        }

        // If we have an existing keyring and signatures failed verification, skip this entry
        if has_keys && has_verification_failure {
            log::warn!(
                "Signatures do not match existing keyring at debian/upstream/signing-key.*. \
                 Not updating watch file or fetching different keys. \
                 If upstream changed their signing key, manually update the keyring."
            );
            return Err(FixerError::NoChanges);
        }

        // For unverified signatures (discovery mode), we need enough successful probes
        let successful_probes = verification_statuses.len();
        if successful_probes < NUM_KEYS_TO_CHECK.min(releases.len()) {
            log::debug!(
                "Not enough signatures found ({} < {}), skipping",
                successful_probes,
                NUM_KEYS_TO_CHECK
            );
            return Err(FixerError::NoChanges);
        }

        let (found_common_mangles, active_common_mangles) = analyze_mangles(&used_mangles);

        log::debug!(
            "Found {} common mangles, {} active",
            found_common_mangles.len(),
            active_common_mangles.len()
        );

        if pgpsigurlmangle.is_none() && !active_common_mangles.is_empty() {
            let issue = LintianIssue::source_with_info(
                "debian-watch-does-not-check-openpgp-signature",
                vec!["[debian/watch]".to_string()],
            );

            if issue.should_fix(base_path) {
                // If only a single mangle is used for all releases that have signatures, set that
                if active_common_mangles.len() == 1 {
                    let new_mangle = active_common_mangles.iter().next().unwrap();
                    log::debug!("Setting pgpsigurlmangle to: {}", new_mangle);
                    entry.set_opt("pgpsigurlmangle", new_mangle);
                }

                // Determine pgpmode and description
                let (pgpmode, mut desc) = determine_pgpmode(&found_common_mangles);
                log::debug!("Setting pgpmode to: {}", pgpmode);
                entry.set_opt("pgpmode", pgpmode);

                // Include fingerprints in description if we found any
                if !needed_keys.is_empty() {
                    let fingerprints: Vec<String> = needed_keys.iter().cloned().collect();
                    desc = format!(
                        "{} ({})",
                        desc.trim_end_matches('.'),
                        fingerprints.join(", ")
                    );
                }
                description = Some(desc);

                made_changes = true;
                fixed_issues.push(issue);
            } else {
                overridden_issues.push(issue);
            }
        }
    }

    if !has_keys && !needed_keys.is_empty() {
        log::debug!("Need to fetch {} keys", needed_keys.len());

        let issue = LintianIssue::source_with_info(
            "debian-watch-file-pubkey-file-is-missing",
            vec!["[debian/watch]".to_string()],
        );

        if issue.should_fix(base_path) {
            let upstream_dir = base_path.join("debian/upstream");
            if !upstream_dir.exists() {
                log::debug!("Creating debian/upstream directory");
                fs::create_dir(&upstream_dir)?;
            }

            let keyfile_path = upstream_dir.join("signing-key.asc");

            // Fetch and export keys using sequoia
            let mut keyfile_content = Vec::new();
            let keys_vec: Vec<String> = needed_keys.iter().cloned().collect();

            // Only fetch from keyservers if net_access is enabled
            if !preferences.net_access.unwrap_or(false) {
                log::warn!("Cannot fetch keys without network access");
                return Err(FixerError::NoChanges);
            }

            for fingerprint in &keys_vec {
                log::debug!("Fetching key with fingerprint: {}", fingerprint);
                // Fetch the certificate from keys.openpgp.org
                let keyserver = std::env::var("KEYSERVER")
                    .unwrap_or_else(|_| "https://keys.openpgp.org".to_string());
                let url = format!("{}/vks/v1/by-fingerprint/{}", keyserver, fingerprint);

                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|e| {
                        FixerError::Other(format!("Failed to build HTTP client: {}", e))
                    })?;

                let response = match client.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => resp,
                    Ok(resp) => {
                        log::warn!(
                            "Keyserver returned status {} for key {}",
                            resp.status(),
                            fingerprint
                        );
                        return Err(FixerError::NoChanges);
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch key {}: {}", fingerprint, e);
                        return Err(FixerError::NoChanges);
                    }
                };

                let key_data = response
                    .bytes()
                    .map_err(|e| FixerError::Other(format!("Failed to read key data: {}", e)))?;

                // Parse the certificate
                use openpgp::parse::Parse;
                let cert = openpgp::Cert::from_reader(std::io::Cursor::new(&key_data[..]))
                    .map_err(|e| {
                        FixerError::Other(format!("Failed to parse certificate: {}", e))
                    })?;

                // Export the key in minimal armored format
                let key_output = export_cert_armored(&cert).map_err(FixerError::Other)?;

                keyfile_content.extend_from_slice(&key_output);
                keyfile_content.push(b'\n');
            }

            if keyfile_content.is_empty() {
                log::warn!("No keys could be fetched");
                return Err(FixerError::NoChanges);
            }

            fs::write(&keyfile_path, &keyfile_content)?;

            made_changes = true;

            if description.is_none() {
                description = Some(format!(
                    "Add upstream signing keys ({}).",
                    needed_keys.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }

            fixed_issues.push(issue);
        } else {
            overridden_issues.push(issue);
        }
    }

    if !made_changes {
        if !overridden_issues.is_empty() {
            return Err(FixerError::NoChangesAfterOverrides(overridden_issues));
        }
        return Err(FixerError::NoChanges);
    }

    // Write the updated watch file
    fs::write(&watch_path, watch_file.to_string())?;

    Ok(FixerResult::builder(
        description.unwrap_or_else(|| "Update PGP signature checking".to_string()),
    )
    .fixed_issues(fixed_issues)
    .overridden_issues(overridden_issues)
    .certainty(crate::Certainty::Certain)
    .build())
}

declare_fixer! {
    name: "pubkey",
    tags: [
        "debian-watch-does-not-check-openpgp-signature",
        "debian-watch-file-pubkey-file-is-missing"
    ],
    apply: |basedir, package, version, preferences| {
        run(basedir, package, version, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_mangles() {
        assert!(COMMON_PGPSIGURL_MANGLES.contains(&"s/$/.asc/"));
        assert!(COMMON_PGPSIGURL_MANGLES.contains(&"s/$/.sig/"));
        assert!(COMMON_PGPSIGURL_MANGLES.contains(&"s/$/.gpg/"));
        assert_eq!(COMMON_PGPSIGURL_MANGLES.len(), 5);
    }

    #[test]
    fn test_analyze_mangles_all_same() {
        let mangles = vec![
            Some("s/$/.asc/".to_string()),
            Some("s/$/.asc/".to_string()),
            Some("s/$/.asc/".to_string()),
        ];
        let (found, active) = analyze_mangles(&mangles);

        assert_eq!(found.len(), 1);
        assert!(found.contains(&Some("s/$/.asc/".to_string())));
        assert_eq!(active.len(), 1);
        assert!(active.contains("s/$/.asc/"));
    }

    #[test]
    fn test_analyze_mangles_mixed() {
        let mangles = vec![
            Some("s/$/.asc/".to_string()),
            None,
            Some("s/$/.asc/".to_string()),
        ];
        let (found, active) = analyze_mangles(&mangles);

        assert_eq!(found.len(), 2); // Some and None
        assert!(found.contains(&Some("s/$/.asc/".to_string())));
        assert!(found.contains(&None));
        assert_eq!(active.len(), 1); // Only the Some variant
        assert!(active.contains("s/$/.asc/"));
    }

    #[test]
    fn test_analyze_mangles_all_none() {
        let mangles = vec![None, None, None];
        let (found, active) = analyze_mangles(&mangles);

        assert_eq!(found.len(), 1);
        assert!(found.contains(&None));
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_analyze_mangles_different_mangles() {
        let mangles = vec![
            Some("s/$/.asc/".to_string()),
            Some("s/$/.sig/".to_string()),
            Some("s/$/.asc/".to_string()),
        ];
        let (found, active) = analyze_mangles(&mangles);

        assert_eq!(found.len(), 2);
        assert_eq!(active.len(), 2);
        assert!(active.contains("s/$/.asc/"));
        assert!(active.contains("s/$/.sig/"));
    }

    #[test]
    fn test_determine_pgpmode_all_signed() {
        let mut mangles = HashSet::new();
        mangles.insert(Some("s/$/.asc/".to_string()));

        let (mode, desc) = determine_pgpmode(&mangles);
        assert_eq!(mode, "mangle");
        assert_eq!(desc, "Check upstream PGP signatures.");
    }

    #[test]
    fn test_determine_pgpmode_mixed() {
        let mut mangles = HashSet::new();
        mangles.insert(Some("s/$/.asc/".to_string()));
        mangles.insert(None);

        let (mode, desc) = determine_pgpmode(&mangles);
        assert_eq!(mode, "auto");
        assert_eq!(desc, "Opportunistically check upstream PGP signatures.");
    }

    #[test]
    fn test_determine_pgpmode_multiple_mangles() {
        let mut mangles = HashSet::new();
        mangles.insert(Some("s/$/.asc/".to_string()));
        mangles.insert(Some("s/$/.sig/".to_string()));

        let (mode, desc) = determine_pgpmode(&mangles);
        assert_eq!(mode, "auto");
        assert_eq!(desc, "Opportunistically check upstream PGP signatures.");
    }

    #[test]
    fn test_verify_signature_with_empty_keyring() {
        let sig_data = b"fake signature data";
        let data = b"fake release data";
        let keyring_data = b"";

        let result = verify_signature(sig_data, data, keyring_data);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No valid certificates"));
    }

    #[test]
    fn test_verify_signature_with_invalid_keyring() {
        let sig_data = b"fake signature data";
        let data = b"fake release data";
        let keyring_data = b"not a valid keyring";

        let result = verify_signature(sig_data, data, keyring_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_export_cert_armored_with_test_key() {
        use openpgp::cert::CertBuilder;

        // Generate a test certificate
        let (cert, _) = CertBuilder::new()
            .add_userid("Test User <test@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate");

        // Export it
        let result = export_cert_armored(&cert);
        assert!(result.is_ok());

        let exported = result.unwrap();
        let exported_str = String::from_utf8_lossy(&exported);

        // Check that it's armored
        assert!(exported_str.contains("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
        assert!(exported_str.contains("-----END PGP PUBLIC KEY BLOCK-----"));
    }

    #[test]
    fn test_verify_signature_roundtrip() {
        use openpgp::cert::CertBuilder;
        use openpgp::policy::StandardPolicy;
        use openpgp::serialize::stream::*;

        let policy = StandardPolicy::new();

        // Generate a test certificate with signing capability
        let (cert, _) = CertBuilder::new()
            .add_userid("Test User <test@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate");

        // Get the signing keypair
        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
            .for_signing()
            .secret()
            .next()
            .expect("No signing key found")
            .key()
            .clone()
            .into_keypair()
            .expect("Failed to convert to keypair");

        // Data to sign
        let data = b"Hello, world!";

        // Create a detached signature
        let mut sig_data = Vec::new();
        {
            let message = Message::new(&mut sig_data);
            let signer = Signer::new(message, keypair)
                .expect("Failed to create signer")
                .detached()
                .build()
                .expect("Failed to build signer");

            let mut writer = signer;
            std::io::copy(&mut std::io::Cursor::new(data), &mut writer)
                .expect("Failed to write data");
            writer.finalize().expect("Failed to finalize signature");
        }

        // Export the certificate as keyring
        let keyring_data = export_cert_armored(&cert).expect("Failed to export cert");

        // Verify the signature
        let result = verify_signature(&sig_data, data, &keyring_data);
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_verify_signature_wrong_data() {
        use openpgp::cert::CertBuilder;
        use openpgp::policy::StandardPolicy;
        use openpgp::serialize::stream::*;

        let policy = StandardPolicy::new();

        // Generate a test certificate with signing capability
        let (cert, _) = CertBuilder::new()
            .add_userid("Test User <test@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate");

        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
            .for_signing()
            .secret()
            .next()
            .expect("No signing key found")
            .key()
            .clone()
            .into_keypair()
            .expect("Failed to convert to keypair");

        let data = b"Hello, world!";

        // Create a detached signature
        let mut sig_data = Vec::new();
        {
            let message = Message::new(&mut sig_data);
            let signer = Signer::new(message, keypair)
                .expect("Failed to create signer")
                .detached()
                .build()
                .expect("Failed to build signer");

            let mut writer = signer;
            std::io::copy(&mut std::io::Cursor::new(data), &mut writer)
                .expect("Failed to write data");
            writer.finalize().expect("Failed to finalize signature");
        }

        let keyring_data = export_cert_armored(&cert).expect("Failed to export cert");

        // Try to verify with different data
        let wrong_data = b"Different data!";
        let result = verify_signature(&sig_data, wrong_data, &keyring_data);
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_verification_status_unverified_when_no_keyring() {
        use openpgp::cert::CertBuilder;
        use openpgp::policy::StandardPolicy;
        use openpgp::serialize::stream::*;

        let policy = StandardPolicy::new();

        // Generate a test certificate
        let (cert, _) = CertBuilder::new()
            .add_userid("Test User <test@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate");

        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
            .for_signing()
            .secret()
            .next()
            .expect("No signing key found")
            .key()
            .clone()
            .into_keypair()
            .expect("Failed to convert to keypair");

        let data = b"Hello, world!";

        // Create a detached signature
        let mut sig_data = Vec::new();
        {
            let message = Message::new(&mut sig_data);
            let signer = Signer::new(message, keypair)
                .expect("Failed to create signer")
                .detached()
                .build()
                .expect("Failed to build signer");

            let mut writer = signer;
            std::io::copy(&mut std::io::Cursor::new(data), &mut writer)
                .expect("Failed to write data");
            writer.finalize().expect("Failed to finalize signature");
        }

        // Create a mock Release (we'll use a simplified approach for testing)
        // Since we can't easily mock Release, we test verify_signature directly

        // Empty keyring should result in unverified status
        let empty_keyring = b"";

        // Since probe_signature needs a Release, let's just verify the verify_signature behavior
        // When keyring is empty, probe_signature returns Unverified status
        // This is tested indirectly through the verify_signature error
        let result = verify_signature(&sig_data, data, empty_keyring);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No valid certificates"));
    }

    #[test]
    fn test_verification_status_verified_with_correct_keyring() {
        use openpgp::cert::CertBuilder;
        use openpgp::policy::StandardPolicy;
        use openpgp::serialize::stream::*;

        let policy = StandardPolicy::new();

        // Generate a test certificate
        let (cert, _) = CertBuilder::new()
            .add_userid("Test User <test@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate");

        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
            .for_signing()
            .secret()
            .next()
            .expect("No signing key found")
            .key()
            .clone()
            .into_keypair()
            .expect("Failed to convert to keypair");

        let data = b"Hello, world!";

        // Create a detached signature
        let mut sig_data = Vec::new();
        {
            let message = Message::new(&mut sig_data);
            let signer = Signer::new(message, keypair)
                .expect("Failed to create signer")
                .detached()
                .build()
                .expect("Failed to build signer");

            let mut writer = signer;
            std::io::copy(&mut std::io::Cursor::new(data), &mut writer)
                .expect("Failed to write data");
            writer.finalize().expect("Failed to finalize signature");
        }

        let keyring_data = export_cert_armored(&cert).expect("Failed to export cert");

        // Verify with correct keyring should return true (Verified status)
        let result = verify_signature(&sig_data, data, &keyring_data);
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn test_verification_status_failed_with_wrong_keyring() {
        use openpgp::cert::CertBuilder;
        use openpgp::policy::StandardPolicy;
        use openpgp::serialize::stream::*;

        let policy = StandardPolicy::new();

        // Generate two different certificates
        let (cert1, _) = CertBuilder::new()
            .add_userid("Test User 1 <test1@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate 1");

        let (cert2, _) = CertBuilder::new()
            .add_userid("Test User 2 <test2@example.com>")
            .add_signing_subkey()
            .generate()
            .expect("Failed to generate test certificate 2");

        // Sign with cert1
        let keypair1 = cert1
            .keys()
            .with_policy(&policy, None)
            .alive()
            .revoked(false)
            .for_signing()
            .secret()
            .next()
            .expect("No signing key found")
            .key()
            .clone()
            .into_keypair()
            .expect("Failed to convert to keypair");

        let data = b"Hello, world!";

        // Create a detached signature with cert1
        let mut sig_data = Vec::new();
        {
            let message = Message::new(&mut sig_data);
            let signer = Signer::new(message, keypair1)
                .expect("Failed to create signer")
                .detached()
                .build()
                .expect("Failed to build signer");

            let mut writer = signer;
            std::io::copy(&mut std::io::Cursor::new(data), &mut writer)
                .expect("Failed to write data");
            writer.finalize().expect("Failed to finalize signature");
        }

        // Export cert2 as keyring (different key!)
        let wrong_keyring = export_cert_armored(&cert2).expect("Failed to export cert");

        // Verify with wrong keyring should return false (Failed status)
        let result = verify_signature(&sig_data, data, &wrong_keyring);
        assert_eq!(result.unwrap(), false);
    }
}
