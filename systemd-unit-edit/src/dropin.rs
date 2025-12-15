//! Drop-in directory support for systemd unit files
//!
//! This module provides functionality for loading and merging systemd drop-in
//! configuration files.

use crate::unit::{Error, SystemdUnit};
use std::path::Path;

impl SystemdUnit {
    /// Load a unit file with drop-in configuration files merged
    ///
    /// This loads the main unit file and then merges all `.conf` files from
    /// the drop-in directory (`<unit>.d/`). Drop-in files are applied in
    /// lexicographic order.
    ///
    /// Drop-in directories are searched in the same directory as the unit file.
    /// For example, if loading `/etc/systemd/system/foo.service`, this will
    /// look for drop-ins in `/etc/systemd/system/foo.service.d/*.conf`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::path::Path;
    /// // Loads foo.service and merges foo.service.d/*.conf
    /// let unit = SystemdUnit::from_file_with_dropins(
    ///     Path::new("/etc/systemd/system/foo.service")
    /// ).unwrap();
    /// ```
    pub fn from_file_with_dropins(path: &Path) -> Result<Self, Error> {
        // Load the main unit file
        let mut unit = Self::from_file(path)?;

        // Determine the drop-in directory path
        let mut dropin_dir = path.to_path_buf();
        dropin_dir.set_extension(format!(
            "{}.d",
            path.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));

        // If the drop-in directory exists, load and merge all .conf files
        if dropin_dir.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&dropin_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("conf"))
                .collect();

            // Sort by filename (lexicographic order)
            entries.sort_by_key(|e| e.file_name());

            // Merge each drop-in file
            for entry in entries {
                let dropin = Self::from_file(&entry.path())?;
                unit.merge_dropin(&dropin);
            }
        }

        Ok(unit)
    }

    /// Merge a drop-in unit file into this unit
    ///
    /// This applies the settings from a drop-in file to the current unit.
    /// According to systemd behavior:
    /// - New sections are added
    /// - Existing keys are replaced with values from the drop-in
    /// - Multiple values for the same key (e.g., `Wants=`) are accumulated
    ///   for directives that support accumulation
    ///
    /// # Example
    ///
    /// ```
    /// # use systemd_unit_edit::SystemdUnit;
    /// # use std::str::FromStr;
    /// let mut main = SystemdUnit::from_str("[Unit]\nDescription=Main\n").unwrap();
    /// let dropin = SystemdUnit::from_str("[Unit]\nAfter=network.target\n").unwrap();
    ///
    /// main.merge_dropin(&dropin);
    ///
    /// let section = main.get_section("Unit").unwrap();
    /// assert_eq!(section.get("Description"), Some("Main".to_string()));
    /// assert_eq!(section.get("After"), Some("network.target".to_string()));
    /// ```
    pub fn merge_dropin(&mut self, dropin: &SystemdUnit) {
        for dropin_section in dropin.sections() {
            let section_name = match dropin_section.name() {
                Some(name) => name,
                None => continue,
            };

            // Find or create the corresponding section in the main unit
            let mut main_section = match self.get_section(&section_name) {
                Some(section) => section,
                None => {
                    // Section doesn't exist, add it
                    self.add_section(&section_name);
                    self.get_section(&section_name).unwrap()
                }
            };

            // Merge entries from the drop-in section
            for entry in dropin_section.entries() {
                let key = match entry.key() {
                    Some(k) => k,
                    None => continue,
                };
                let value = match entry.value() {
                    Some(v) => v,
                    None => continue,
                };

                // For accumulating directives (like Wants, After, etc.),
                // add rather than replace. For others, replace.
                if crate::systemd_metadata::is_accumulating_directive(&key) {
                    main_section.add(&key, &value);
                } else {
                    main_section.set(&key, &value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_merge_dropin_basic() {
        let mut main = SystemdUnit::from_str("[Unit]\nDescription=Main\n").unwrap();
        let dropin = SystemdUnit::from_str("[Unit]\nAfter=network.target\n").unwrap();

        main.merge_dropin(&dropin);

        let section = main.get_section("Unit").unwrap();
        assert_eq!(section.get("Description"), Some("Main".to_string()));
        assert_eq!(section.get("After"), Some("network.target".to_string()));
    }

    #[test]
    fn test_merge_dropin_replaces_non_accumulating() {
        let mut main = SystemdUnit::from_str("[Unit]\nDescription=Main\n").unwrap();
        let dropin = SystemdUnit::from_str("[Unit]\nDescription=Updated\n").unwrap();

        main.merge_dropin(&dropin);

        let section = main.get_section("Unit").unwrap();
        assert_eq!(section.get("Description"), Some("Updated".to_string()));
    }

    #[test]
    fn test_merge_dropin_accumulates() {
        let mut main =
            SystemdUnit::from_str("[Unit]\nWants=foo.service\nAfter=foo.service\n").unwrap();
        let dropin =
            SystemdUnit::from_str("[Unit]\nWants=bar.service\nAfter=bar.service\n").unwrap();

        main.merge_dropin(&dropin);

        let section = main.get_section("Unit").unwrap();
        let wants = section.get_all("Wants");
        assert_eq!(wants.len(), 2);
        assert!(wants.contains(&"foo.service".to_string()));
        assert!(wants.contains(&"bar.service".to_string()));

        let after = section.get_all("After");
        assert_eq!(after.len(), 2);
        assert!(after.contains(&"foo.service".to_string()));
        assert!(after.contains(&"bar.service".to_string()));
    }

    #[test]
    fn test_merge_dropin_new_section() {
        let mut main = SystemdUnit::from_str("[Unit]\nDescription=Main\n").unwrap();
        let dropin = SystemdUnit::from_str("[Service]\nType=simple\n").unwrap();

        main.merge_dropin(&dropin);

        assert_eq!(main.sections().count(), 2);
        let service = main.get_section("Service").unwrap();
        assert_eq!(service.get("Type"), Some("simple".to_string()));
    }

    #[test]
    fn test_merge_dropin_mixed() {
        let mut main = SystemdUnit::from_str(
            "[Unit]\nDescription=Main\nWants=foo.service\n\n[Service]\nType=simple\n",
        )
        .unwrap();
        let dropin = SystemdUnit::from_str(
            "[Unit]\nAfter=network.target\nWants=bar.service\n\n[Service]\nRestart=always\n",
        )
        .unwrap();

        main.merge_dropin(&dropin);

        let unit_section = main.get_section("Unit").unwrap();
        assert_eq!(unit_section.get("Description"), Some("Main".to_string()));
        assert_eq!(
            unit_section.get("After"),
            Some("network.target".to_string())
        );
        let wants = unit_section.get_all("Wants");
        assert_eq!(wants.len(), 2);

        let service_section = main.get_section("Service").unwrap();
        assert_eq!(service_section.get("Type"), Some("simple".to_string()));
        assert_eq!(service_section.get("Restart"), Some("always".to_string()));
    }
}
