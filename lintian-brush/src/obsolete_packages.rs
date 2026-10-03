//! Obsolete packages known to lintian.

include!(concat!(env!("OUT_DIR"), "/obsolete_packages.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_contains_known_entries() {
        // Sanity check that the generated table actually holds entries
        // from data/fields/obsolete-packages.
        assert_eq!(obsolete_package_replacement("exim"), Some(Some("exim4")));
        assert_eq!(
            obsolete_package_replacement("mysql-client"),
            Some(Some("default-mysql-client"))
        );
        // Present but no replacement listed.
        assert!(obsolete_package_replacement("cdrecord").is_some());
        // Not obsolete at all.
        assert_eq!(obsolete_package_replacement("libc6"), None);
    }
}
