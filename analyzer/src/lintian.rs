//! Lintian data structures and utilities

/// The path to the Lintian data directory
pub const LINTIAN_DATA_PATH: &str = "/usr/share/lintian/data";

/// The path to the Lintian tags file
pub const RELEASE_DATES_PATH: &str = "/usr/share/lintian/data/debian-policy/release-dates.json";

#[derive(Debug, Clone, serde::Deserialize)]
/// A release of the Debian Policy
pub struct PolicyRelease {
    /// The version of the release
    pub version: StandardsVersion,
    /// When the release was published
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// List of bug numbers closed by this release
    pub closes: Vec<i32>,
    /// The epoch of the release
    pub epoch: Option<i32>,
    /// The author of the release
    pub author: Option<String>,
    /// The changes made in this release
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct Preamble {
    pub cargo: String,
    pub title: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct PolicyReleases {
    pub preamble: Preamble,
    pub releases: Vec<PolicyRelease>,
}

#[derive(Debug, Clone)]
/// A version of the Debian Policy
pub struct StandardsVersion(Vec<i32>);

impl StandardsVersion {
    fn normalize(&self, n: usize) -> Self {
        let mut version = self.0.clone();
        version.resize(n, 0);
        Self(version)
    }
}

impl std::cmp::PartialEq for StandardsVersion {
    fn eq(&self, other: &Self) -> bool {
        // Normalize to the same length
        let n = std::cmp::max(self.0.len(), other.0.len());
        let self_normalized = self.normalize(n);
        let other_normalized = other.normalize(n);
        self_normalized.0 == other_normalized.0
    }
}

impl std::cmp::Ord for StandardsVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Normalize to the same length
        let n = std::cmp::max(self.0.len(), other.0.len());
        let self_normalized = self.normalize(n);
        let other_normalized = other.normalize(n);
        self_normalized.0.cmp(&other_normalized.0)
    }
}

impl std::cmp::PartialOrd for StandardsVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Eq for StandardsVersion {}

impl std::str::FromStr for StandardsVersion {
    type Err = core::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.').map(|part| part.parse::<i32>());
        let mut version = Vec::new();
        for part in &mut parts {
            version.push(part?);
        }
        Ok(StandardsVersion(version))
    }
}

impl<'a> serde::Deserialize<'a> for StandardsVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for StandardsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
                .join(".")
        )
    }
}

/// Returns an iterator over all known standards versions
pub fn iter_standards_versions() -> impl Iterator<Item = PolicyRelease> {
    let data = std::fs::read(RELEASE_DATES_PATH).expect("Failed to read release dates");
    let data: PolicyReleases =
        serde_json::from_slice(&data).expect("Failed to parse release dates");
    data.releases.into_iter()
}

/// Returns the latest standards version
pub fn latest_standards_version() -> StandardsVersion {
    iter_standards_versions()
        .next()
        .expect("No standards versions found")
        .version
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_standards_version() {
        let version: super::StandardsVersion = "4.2.0".parse().unwrap();
        assert_eq!(version.0, vec![4, 2, 0]);
        assert_eq!(version.to_string(), "4.2.0");
        assert_eq!(version, "4.2".parse().unwrap());
        assert_eq!(version, "4.2.0".parse().unwrap());
    }

    #[test]
    fn test_parse_releases() {
        let input = r###"{
   "preamble" : {
      "cargo" : "releases",
      "title" : "Debian Policy Releases"
   },
   "releases" : [
      {
         "author" : "Sean Whitton <spwhitton@spwhitton.name>",
         "changes" : [
            "",
            "debian-policy (4.7.0.0) unstable; urgency=medium",
            "",
            "  [ Sean Whitton ]",
            "  * Policy: Prefer native overriding mechanisms to diversions & alternatives",
            "    Wording: Luca Boccassi <bluca@debian.org>",
            "    Seconded: Sean Whitton <spwhitton@spwhitton.name>",
            "    Seconded: Russ Allbery <rra@debian.org>",
            "    Seconded: Holger Levsen <holger@layer-acht.org>",
            "    Closes: #1035733",
            "  * Policy: Improve alternative build dependency discussion",
            "    Wording: Russ Allbery <rra@debian.org>",
            "    Seconded: Wouter Verhelst <wouter@debian.org>",
            "    Seconded: Sean Whitton <spwhitton@spwhitton.name>",
            "    Closes: #968226",
            "  * Policy: No network access for required targets for contrib & non-free",
            "    Wording: Aurelien Jarno <aurel32@debian.org>",
            "    Seconded: Sam Hartman <hartmans@debian.org>",
            "    Seconded: Tobias Frost <tobi@debian.org>",
            "    Seconded: Holger Levsen <holger@layer-acht.org>",
            "    Closes: #1068192",
            "",
            "  [ Russ Allbery ]",
            "  * Policy: Add mention of the new non-free-firmware archive area",
            "    Wording: Gunnar Wolf <gwolf@gwolf.org>",
            "    Seconded: Holger Levsen <holger@layer-acht.org>",
            "    Seconded: Russ Allbery <rra@debian.org>",
            "    Closes: #1029211",
            "  * Policy: Source packages in main may build binary packages in contrib",
            "    Wording: Simon McVittie <smcv@debian.org>",
            "    Seconded: Holger Levsen <holger@layer-acht.org>",
            "    Seconded: Russ Allbery <rra@debian.org>",
            "    Closes: #994008",
            "  * Policy: Allow hard links in source packages",
            "    Wording: Russ Allbery <rra@debian.org>",
            "    Seconded: Helmut Grohne <helmut@subdivi.de>",
            "    Seconded: Guillem Jover <guillem@debian.org>",
            "    Closes: #970234",
            "  * Policy: Binary and Description fields may be absent in .changes",
            "    Wording: Russ Allbery <rra@debian.org>",
            "    Seconded: Sam Hartman <hartmans@debian.org>",
            "    Seconded: Guillem Jover <guillem@debian.org>",
            "    Closes: #963524",
            "  * Policy: systemd units are required to start and stop system services",
            "    Wording: Luca Boccassi <bluca@debian.org>",
            "    Wording: Russ Allbery <rra@debian.org>",
            "    Seconded: Luca Boccassi <bluca@debian.org>",
            "    Seconded: Sam Hartman <hartmans@debian.org>",
            "    Closes: #1039102"
         ],
         "closes" : [
            963524,
            968226,
            970234,
            994008,
            1029211,
            1035733,
            1039102,
            1068192
         ],
         "epoch" : 1712466535,
         "timestamp" : "2024-04-07T05:08:55Z",
         "version" : "4.7.0.0"
      }
   ]
}"###;
        let data: super::PolicyReleases = serde_json::from_str(input).unwrap();
        assert_eq!(data.releases.len(), 1);
    }
}
