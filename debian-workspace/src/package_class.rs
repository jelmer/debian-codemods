//! Heuristics for classifying binary packages.
//!
//! Mirrors lintian's `Lintian::Processable::Installable::Class` so that
//! callers across the workspace can share the same definitions.

const META_DESCRIPTION_NEEDLES: &[&str] = &[
    "metapackage",
    "meta package",
    "meta-package",
    "dependency package",
    "dummy package",
    "empty package",
];

const META_SECTIONS: &[&str] = &["tasks", "metapackages"];

/// Returns true if the binary package is probably transitional.
///
/// Mirrors lintian's `is_transitional`: case-insensitive match of
/// `transitional package` anywhere in the description.
pub fn is_transitional(description: &str) -> bool {
    description
        .to_ascii_lowercase()
        .contains("transitional package")
}

/// Returns true if the binary package is probably a meta or task package.
///
/// Mirrors lintian's `is_meta_package`. Returns true when:
/// - the description (case-insensitively) contains `metapackage`,
///   `meta package`, `meta-package`, or `(dependency|dummy|empty) package`; or
/// - the section is `tasks` or `metapackages`, optionally prefixed by an
///   archive area (e.g. `contrib/metapackages`); or
/// - the package name starts with `task-`.
pub fn is_meta_package(name: &str, description: &str, section: Option<&str>) -> bool {
    let lower_description = description.to_ascii_lowercase();
    if META_DESCRIPTION_NEEDLES
        .iter()
        .any(|needle| lower_description.contains(needle))
    {
        return true;
    }
    if let Some(section) = section {
        let tail = section.rsplit('/').next().unwrap_or(section);
        if META_SECTIONS.contains(&tail) {
            return true;
        }
    }
    if name.starts_with("task-") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitional_matches_description() {
        assert!(is_transitional("This is a transitional package."));
        assert!(is_transitional("Transitional Package for foo"));
    }

    #[test]
    fn transitional_does_not_match_other_text() {
        assert!(!is_transitional("Regular package"));
        assert!(!is_transitional("transition helper"));
    }

    #[test]
    fn meta_matches_description_metapackage() {
        assert!(is_meta_package("foo", "metapackage for foo", None));
        assert!(is_meta_package("foo", "Meta-Package for foo", None));
        assert!(is_meta_package("foo", "Meta package for foo", None));
    }

    #[test]
    fn meta_matches_description_dummy_variants() {
        assert!(is_meta_package("foo", "dependency package", None));
        assert!(is_meta_package(
            "foo",
            "Dummy Package pulling in bits",
            None
        ));
        assert!(is_meta_package("foo", "Empty package", None));
    }

    #[test]
    fn meta_matches_section() {
        assert!(is_meta_package("foo", "regular", Some("metapackages")));
        assert!(is_meta_package("foo", "regular", Some("tasks")));
        assert!(is_meta_package(
            "foo",
            "regular",
            Some("contrib/metapackages")
        ));
    }

    #[test]
    fn meta_matches_task_prefix() {
        assert!(is_meta_package("task-desktop", "regular", None));
    }

    #[test]
    fn meta_negative() {
        assert!(!is_meta_package("foo", "regular package", Some("libs")));
        assert!(!is_meta_package("foo", "regular package", None));
        // section ending in a path that happens to suffix-match — must not
        // false-positive on something like "subtasks".
        assert!(!is_meta_package("foo", "regular", Some("subtasks")));
    }
}
