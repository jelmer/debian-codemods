use debian_control::control::MultiArch;
use std::collections::{HashMap, HashSet};
use toml_edit::{value, DocumentMut, Table};

pub const DEFAULT_MAINTAINER: &str =
    "Debian Rust Maintainers <pkg-rust-maintainers@alioth-lists.debian.net>";
pub const DEFAULT_SECTION: &str = "rust";
pub const CURRENT_STANDARDS_VERSION: &str = "4.5.1";
pub const DEFAULT_PRIORITY: debian_control::Priority = debian_control::Priority::Optional;

pub struct DebcargoEditor {
    debcargo: DocumentMut,
    cargo: Option<DocumentMut>,
}

impl From<DocumentMut> for DebcargoEditor {
    fn from(doc: DocumentMut) -> Self {
        Self {
            debcargo: doc,
            cargo: None,
        }
    }
}

impl DebcargoEditor {
    pub fn new() -> Self {
        Self {
            debcargo: DocumentMut::new(),
            cargo: None,
        }
    }

    pub fn from_string(s: &str) -> Result<Self, toml_edit::TomlError> {
        Ok(Self {
            debcargo: s.parse()?,
            cargo: None,
        })
    }

    fn crate_name(&self) -> Option<&str> {
        self.cargo
            .as_ref()
            .and_then(|c| c["package"]["name"].as_str())
    }

    fn crate_version(&self) -> Option<semver::Version> {
        self.cargo
            .as_ref()
            .and_then(|c| c["package"]["version"].as_str())
            .map(|s| semver::Version::parse(s).unwrap())
    }

    pub fn open(&self, path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::from_string(&content).unwrap())
    }

    pub fn from_directory(&self, path: &str) -> Result<Self, std::io::Error> {
        let debcargo_toml = std::fs::read_to_string(format!("{}/debian/debcargo.toml", path))?;
        let cargo_toml = std::fs::read_to_string(format!("{}/Cargo.toml", path))?;
        Ok(Self {
            debcargo: debcargo_toml.parse().unwrap(),
            cargo: Some(cargo_toml.parse().unwrap()),
        })
    }

    pub fn source(&mut self) -> DebcargoSource {
        DebcargoSource { main: self }
    }

    fn semver_suffix(&self) -> bool {
        self.debcargo["source"]
            .get("semver_suffix")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn binaries(&mut self) -> impl Iterator<Item = DebcargoBinary<'_>> {
        let semver_suffix = self.semver_suffix();

        let mut ret: HashMap<String, String> = HashMap::new();
        ret.insert(
            debcargo_binary_name(
                self.crate_name().unwrap(),
                &if semver_suffix {
                    semver_pair(&self.crate_version().unwrap())
                } else {
                    "".to_string()
                },
            ),
            "lib".to_string(),
        );

        if self.debcargo["bin"].as_bool().unwrap_or(!semver_suffix) {
            let bin_name = self.debcargo["bin_name"]
                .as_str()
                .unwrap_or_else(|| self.crate_name().unwrap());
            ret.insert(bin_name.to_owned(), "bin".to_string());
        }

        let global_summary = self.global_summary();
        let global_description = self.global_description();
        let crate_name = self.crate_name().unwrap().to_string();
        let crate_version = self.crate_version().unwrap();
        let features = self.features();

        self.debcargo
            .as_table_mut()
            .iter_mut()
            .filter_map(move |(key, item)| {
                let kind = ret.remove(&key.to_string())?;
                Some(DebcargoBinary::new(
                    kind,
                    key.to_string(),
                    item.as_table_mut().unwrap(),
                    global_summary.clone(),
                    global_description.clone(),
                    crate_name.clone(),
                    crate_version.clone(),
                    semver_suffix,
                    features.clone(),
                ))
            })
    }

    fn global_summary(&self) -> Option<String> {
        if let Some(summary) = self.debcargo.get("summary").and_then(|v| v.as_str()) {
            Some(format!("{} - Rust source code", summary))
        } else {
            self.cargo.as_ref().and_then(|c| {
                c["package"]
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.split('\n').next().unwrap().to_string())
            })
        }
    }

    fn global_description(&self) -> Option<String> {
        self.debcargo
            .get("description")
            .and_then(|v| v.as_str())
            .map(|description| description.to_owned())
    }

    fn features(&self) -> Option<HashSet<String>> {
        self.cargo
            .as_ref()
            .and_then(|c| c["features"].as_table())
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
    }
}

pub struct DebcargoSource<'a> {
    main: &'a mut DebcargoEditor,
}

impl<'a> DebcargoSource<'a> {
    pub fn set_standards_version(&mut self, version: &str) -> &mut Self {
        self.main.debcargo["source"]["standards-version"] = value(version);
        self
    }

    pub fn standards_version(&self) -> &str {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("standards-version"))
            .and_then(|v| v.as_str())
            .unwrap_or(CURRENT_STANDARDS_VERSION)
    }

    pub fn set_homepage(&mut self, homepage: &str) -> &mut Self {
        self.main.debcargo["source"]["homepage"] = value(homepage);
        self
    }

    pub fn homepage(&self) -> Option<&str> {
        let default_homepage = self
            .main
            .cargo
            .as_ref()
            .and_then(|c| c.get("package"))
            .and_then(|x| x.get("homepage"))
            .and_then(|v| v.as_str());
        self.main.debcargo["source"]["homepage"]
            .as_str()
            .or(default_homepage)
    }

    pub fn set_vcs_git(&mut self, git: &str) -> &mut Self {
        self.main.debcargo["source"]["vcs_git"] = value(git);
        self
    }

    pub fn vcs_git(&self) -> Option<String> {
        let default_git = self.main.crate_name().map(|c| {
            format!(
                "https://salsa.debian.org/rust-team/debcargo-conf.git [src/{}]",
                c.to_lowercase()
            )
        });

        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("vcs_git"))
            .and_then(|v| v.as_str())
            .map_or(default_git, |s| Some(s.to_string()))
    }

    pub fn vcs_browser(&self) -> Option<String> {
        let default_vcs_browser = self.main.crate_name().map(|c| {
            format!(
                "https://salsa.debian.org/rust-team/debcargo-conf/tree/master/src/{}",
                c.to_lowercase()
            )
        });

        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("vcs_browser"))
            .and_then(|v| v.as_str())
            .map_or(default_vcs_browser, |s| Some(s.to_string()))
    }

    pub fn set_vcs_browser(&mut self, browser: &str) -> &mut Self {
        self.main.debcargo["source"]["vcs_browser"] = value(browser);
        self
    }

    pub fn section(&self) -> &str {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("section"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_SECTION)
    }

    pub fn set_section(&mut self, section: &str) -> &mut Self {
        self.main.debcargo["source"]["section"] = value(section);
        self
    }

    pub fn name(&self) -> Option<String> {
        let semver_suffix = self.main.semver_suffix();
        if semver_suffix {
            let crate_name = self.main.crate_name().map(debnormalize);
            Some(format!(
                "rust-{}-{}",
                crate_name?,
                semver_pair(&self.main.crate_version()?)
            ))
        } else {
            Some(format!("rust-{}", debnormalize(self.main.crate_name()?)))
        }
    }

    pub fn priority(&self) -> debian_control::Priority {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("priority"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PRIORITY)
    }

    pub fn set_priority(&mut self, priority: debian_control::Priority) -> &mut Self {
        self.main.debcargo["source"]["priority"] = value(priority.to_string());
        self
    }

    pub fn rules_requires_root(&self) -> bool {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("requires_root"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn set_rules_requires_root(&mut self, requires_root: bool) -> &mut Self {
        self.main.debcargo["source"]["requires_root"] =
            value(if requires_root { "yes" } else { "no" });
        self
    }

    pub fn maintainer(&self) -> &str {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("maintainer"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_MAINTAINER)
    }

    pub fn set_maintainer(&mut self, maintainer: &str) -> &mut Self {
        self.main.debcargo["source"]["maintainer"] = value(maintainer);
        self
    }

    pub fn uploaders(&self) -> Option<Vec<String>> {
        self.main
            .debcargo
            .get("source")
            .and_then(|s| s.get("uploaders"))
            .and_then(|x| x.as_array())
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
    }

    pub fn set_uploaders(&mut self, uploaders: Vec<String>) -> &mut Self {
        let mut array = toml_edit::Array::new();
        for u in uploaders {
            array.push(u);
        }
        self.main.debcargo["source"]["uploaders"] = value(array);
        self
    }
}

pub struct DebcargoBinary<'a> {
    table: &'a mut Table,
    key: String,
    name: String,
    section: String,
    global_summary: Option<String>,
    global_description: Option<String>,
    crate_name: String,
    crate_version: semver::Version,
    semver_suffix: bool,
    features: Option<HashSet<String>>,
}

impl<'a> DebcargoBinary<'a> {
    fn new(
        key: String,
        name: String,
        table: &'a mut Table,
        global_summary: Option<String>,
        global_description: Option<String>,
        crate_name: String,
        crate_version: semver::Version,
        semver_suffix: bool,
        features: Option<HashSet<String>>,
    ) -> Self {
        Self {
            key: key.to_owned(),
            name,
            section: format!("packages.{}", key),
            table,
            global_summary,
            global_description,
            crate_name,
            crate_version,
            semver_suffix,
            features,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn architecture(&self) -> Option<&str> {
        Some("any")
    }

    pub fn multi_arch(&self) -> Option<MultiArch> {
        Some(MultiArch::Same)
    }

    pub fn section(&self) -> Option<&str> {
        self.table["section"].as_str()
    }

    pub fn summary(&self) -> Option<String> {
        if let Some(summary) = self.table.get("summary").and_then(|v| v.as_str()) {
            Some(summary.to_string())
        } else {
            self.global_summary.clone()
        }
    }

    pub fn long_description(&self) -> Option<String> {
        if let Some(description) = self.table.get("description").and_then(|v| v.as_str()) {
            Some(description.to_string())
        } else if let Some(description) = self.global_description.as_ref() {
            Some(description.to_string())
        } else {
            match self.key.as_str() {
                "lib" => Some(format!("Source code for Debianized Rust crate \"{}\"", self.crate_name)),
                "bin" => Some("This package contains the source for the Rust mio crate, packaged by debcargo for use with cargo and dh-cargo.".to_owned()),
                _ => None,
            }
        }
    }

    pub fn description(&self) -> Option<String> {
        Some(crate::control::format_description(
            &self.summary()?,
            self.long_description()?.split('\n').collect(),
        ))
    }

    pub fn depends(&self) -> Option<&str> {
        self.table["depends"].as_str()
    }

    pub fn recommends(&self) -> Option<&str> {
        self.table["recommends"].as_str()
    }

    pub fn suggests(&self) -> Option<&str> {
        self.table["suggests"].as_str()
    }

    fn default_provides(&self) -> Option<String> {
        let mut ret = HashSet::new();
        let semver_suffix = self.semver_suffix;
        let semver = &self.crate_version;

        let mut suffixes = vec![];
        if !semver_suffix {
            suffixes.push("".to_string());
        }

        suffixes.push(format!("-{}", semver.major));
        suffixes.push(format!("-{}.{}", semver.major, semver.minor));
        suffixes.push(format!(
            "-{}.{}.{}",
            semver.major, semver.minor, semver.patch
        ));
        for ver_suffix in suffixes {
            let mut feature_suffixes = HashSet::new();
            feature_suffixes.insert("".to_string());
            feature_suffixes.insert("+default".to_string());
            feature_suffixes.extend(
                self.features
                    .as_ref()
                    .map(|k| k.iter().map(|k| format!("+{}", k)).collect::<HashSet<_>>())
                    .unwrap_or_default(),
            );
            for feature_suffix in feature_suffixes {
                ret.insert(debcargo_binary_name(
                    &self.crate_name,
                    &format!("{}{}", ver_suffix, &feature_suffix),
                ));
            }
        }
        ret.remove(self.name());
        if ret.is_empty() {
            None
        } else {
            Some(format!(
                "\n{}",
                &ret.iter()
                    .map(|s| format!("{} (= ${{binary:Version}})", s))
                    .collect::<Vec<_>>()
                    .join(",\n ")
            ))
        }
    }
}

fn debnormalize(s: &str) -> String {
    s.to_lowercase().replace('_', "-")
}

fn semver_pair(s: &semver::Version) -> String {
    format!("{}.{}", s.major, s.minor)
}

fn debcargo_binary_name(crate_name: &str, suffix: &str) -> String {
    format!("librust-{}{}-dev", debnormalize(crate_name), suffix)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_debcargo_binary_name() {
        assert_eq!(super::debcargo_binary_name("foo", ""), "librust-foo-dev");
        assert_eq!(
            super::debcargo_binary_name("foo", "-1"),
            "librust-foo-1-dev"
        );
        assert_eq!(
            super::debcargo_binary_name("foo", "-1.2"),
            "librust-foo-1.2-dev"
        );
        assert_eq!(
            super::debcargo_binary_name("foo", "-1.2.3"),
            "librust-foo-1.2.3-dev"
        );
    }

    #[test]
    fn test_semver_pair() {
        assert_eq!(super::semver_pair("1.2.3"), "1.2");
        assert_eq!(super::semver_pair("1.2.6"), "1.2");
    }

    #[test]
    fn test_debnormalize() {
        assert_eq!(super::debnormalize("foo_bar"), "foo-bar");
        assert_eq!(super::debnormalize("foo"), "foo");
    }

    #[test]
    fn test_debcargo_editor() {
        let mut editor = super::DebcargoEditor::new();
        editor.debcargo["source"]["standards-version"] = toml_edit::value("4.5.1");
        editor.debcargo["source"]["homepage"] = toml_edit::value("https://example.com");
        editor.debcargo["source"]["vcs_git"] = toml_edit::value("https://example.com");
        editor.debcargo["source"]["vcs_browser"] = toml_edit::value("https://example.com");
        editor.debcargo["source"]["section"] = toml_edit::value("rust");
        editor.debcargo["source"]["priority"] = toml_edit::value("optional");
        editor.debcargo["source"]["requires_root"] = toml_edit::value("no");
        editor.debcargo["source"]["maintainer"] =
            toml_edit::value("Jelmer Vernooij <jelmer@debian.org>");

        assert_eq!(editor.source().standards_version(), "4.5.1");
        assert_eq!(editor.source().homepage(), Some("https://example.com"));
        assert_eq!(
            editor.source().vcs_git().as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            editor.source().vcs_browser().as_deref(),
            Some("https://example.com")
        );
        assert_eq!(editor.source().section(), "rust");
        assert_eq!(editor.source().priority(), super::DEFAULT_PRIORITY);
        assert!(!editor.source().rules_requires_root());
        assert_eq!(
            editor.source().maintainer(),
            "Jelmer Vernooij <jelmer@debian.org>"
        );
        assert_eq!(editor.source().name(), None);
        assert_eq!(editor.source().uploaders(), None);
    }
}
