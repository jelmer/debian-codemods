//! Lintian-brush configuration file.
use crate::Certainty;
use configparser::ini::Ini;
use log::warn;

const SUPPORTED_KEYS: &[&str] = &[
    "compat-release",
    "minimum-certainty",
    "allow-reformatting",
    "update-changelog",
];

pub const PACKAGE_CONFIG_FILENAME: &str = "debian/lintian-brush.conf";

pub struct Config {
    obj: Ini,
}

impl Config {
    pub fn load_from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let mut ini = Ini::new();
        let data = std::fs::read_to_string(path)?;
        ini.read(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        for (section, contents) in ini.get_map_ref() {
            if section != "default" {
                warn!(
                    "unknown section {} in {}, ignoring.",
                    section,
                    path.display()
                );
                continue;
            }
            for key in contents.keys() {
                if !SUPPORTED_KEYS.contains(&key.as_str()) {
                    warn!(
                        "unknown key {} in section {} in {}, ignoring.",
                        key,
                        section,
                        path.display()
                    );

                    continue;
                }
            }
        }

        Ok(Config { obj: ini })
    }

    pub fn compat_release(&self) -> Option<String> {
        if let Some(value) = self.obj.get("default", "compat-release") {
            let codename = crate::release_info::resolve_release_codename(value.as_str(), None);
            if codename.is_none() {
                warn!("unknown compat release {}, ignoring.", value);
            }
            codename
        } else {
            None
        }
    }

    pub fn allow_reformatting(&self) -> Option<bool> {
        match self.obj.getbool("default", "allow-reformatting") {
            Ok(value) => value,
            Err(e) => {
                warn!("invalid allow-reformatting value {}, ignoring.", e);
                None
            }
        }
    }

    pub fn minimum_certainty(&self) -> Option<Certainty> {
        self.obj
            .get("default", "minimum-certainty")
            .and_then(|value| {
                value
                    .parse::<Certainty>()
                    .map_err(|e| {
                        warn!("invalid minimum-certainty value {}, ignoring.", value);
                        e
                    })
                    .ok()
            })
    }

    pub fn update_changelog(&self) -> Option<bool> {
        match self.obj.getbool("default", "update-changelog") {
            Ok(value) => value,
            Err(e) => {
                warn!("invalid update-changelog value {}, ignoring.", e);
                None
            }
        }
    }
}
