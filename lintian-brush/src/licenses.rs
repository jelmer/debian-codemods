use std::collections::HashMap;

/// Path to the directory containing common license files on Debian systems
pub const COMMON_LICENSES_DIR: &str = "/usr/share/common-licenses";

lazy_static::lazy_static! {
    /// Mapping of SPDX license identifiers to their full license names
    pub static ref FULL_LICENSE_NAME: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();
        m.insert("Apache-2.0", "Apache License, Version 2.0");
        m.insert("CC0-1.0", "CC0 1.0 Universal license");
        m
    };
}
