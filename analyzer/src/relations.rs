use debian_control::lossless::relations::{Entry, Relation, Relations};
use debian_control::relations::VersionConstraint;
use debversion::Version;

/// Check if one dependency is implied by another.
///
/// Is dep implied by outer?
pub fn is_dep_implied(dep: &Relation, outer: &Relation) -> bool {
    if dep.name() != outer.name() {
        return false;
    }

    let (v1, v2) = match (dep.version(), outer.version()) {
        (Some(v1), Some(v2)) => (v1, v2),
        (None, _) => return true,
        (_, None) => return false,
    };

    match (v1, v2) {
        ((VersionConstraint::GreaterThanEqual, v1), (VersionConstraint::GreaterThan, v2)) => {
            v2 > v1
        }
        (
            (VersionConstraint::GreaterThanEqual, v1),
            (VersionConstraint::GreaterThanEqual, v2) | (VersionConstraint::Equal, v2),
        ) => v2 >= v1,
        (
            (VersionConstraint::GreaterThanEqual, _v1),
            (VersionConstraint::LessThanEqual, _v2) | (VersionConstraint::LessThan, _v2),
        ) => false,
        ((VersionConstraint::Equal, v1), (VersionConstraint::Equal, v2)) => v2 == v1,
        ((VersionConstraint::Equal, _), (_, _)) => false,
        ((VersionConstraint::LessThan, v1), (VersionConstraint::LessThan, v2)) => v2 <= v1,
        (
            (VersionConstraint::LessThan, v1),
            (VersionConstraint::LessThanEqual, v2) | (VersionConstraint::Equal, v2),
        ) => v2 < v1,
        (
            (VersionConstraint::LessThan, _v1),
            (VersionConstraint::GreaterThanEqual, _v2) | (VersionConstraint::GreaterThan, _v2),
        ) => false,
        (
            (VersionConstraint::LessThanEqual, v1),
            (VersionConstraint::LessThanEqual, v2)
            | (VersionConstraint::Equal, v2)
            | (VersionConstraint::LessThan, v2),
        ) => v2 <= v1,
        (
            (VersionConstraint::LessThanEqual, _v1),
            (VersionConstraint::GreaterThanEqual, _v2) | (VersionConstraint::GreaterThan, _v2),
        ) => false,
        ((VersionConstraint::GreaterThan, v1), (VersionConstraint::GreaterThan, v2)) => v2 >= v1,
        (
            (VersionConstraint::GreaterThan, v1),
            (VersionConstraint::GreaterThanEqual, v2) | (VersionConstraint::Equal, v2),
        ) => v2 > v1,
        (
            (VersionConstraint::GreaterThan, _v1),
            (VersionConstraint::LessThanEqual, _v2) | (VersionConstraint::LessThan, _v2),
        ) => false,
    }
}

/// Check if one relation implies another.
///
/// # Arguments
/// * `inner` - Inner relation
/// * `outer` - Outer relation
pub fn is_relation_implied(inner: &Entry, outer: &Entry) -> bool {
    if inner == outer {
        return true;
    }

    // "bzr >= 1.3" implied by "bzr >= 1.3 | libc6"
    for inner_dep in inner.relations() {
        if outer
            .relations()
            .any(|outer_dep| is_dep_implied(&inner_dep, &outer_dep))
        {
            return true;
        }
    }

    false
}

/// Ensure that a relation exists.
///
/// This is done by either verifying that there is an existing
/// relation that satisfies the specified relation, or
/// by upgrading an existing relation.
pub fn ensure_relation(rels: &mut Relations, newrel: Entry) {
    let mut obsolete = vec![];
    for (i, relation) in rels.entries().enumerate() {
        if is_relation_implied(&newrel, &relation) {
            // The new relation is already implied by an existing one.
            return;
        }
        if is_relation_implied(&relation, &newrel) {
            obsolete.push(i);
        }
    }

    if let Some(pos) = obsolete.pop() {
        rels.replace(pos, newrel);
    } else {
        rels.push(newrel);
    }

    for i in obsolete.into_iter().rev() {
        rels.remove(i);
    }
}

/// Update a relation string to ensure a particular version is required.
///
/// # Arguments
/// * `relations` - Package relation
/// * `package` - Package name
/// * `minimum_version` - Minimum version
///
/// # Returns
/// True if the relation was changed
pub fn ensure_minimum_version(
    relations: &mut Relations,
    package: &str,
    minimum_version: &Version,
) -> bool {
    let is_obsolete = |entry: &Entry| -> bool {
        for r in entry.relations() {
            if r.name() != package {
                continue;
            }
            if let Some((vc, v)) = r.version().as_ref() {
                if *vc == VersionConstraint::GreaterThan && v < minimum_version {
                    return true;
                }
                if *vc == VersionConstraint::GreaterThanEqual && v <= minimum_version {
                    return true;
                }
            }
        }
        false
    };

    let mut found = false;
    let mut changed = false;
    let mut obsolete_relations = vec![];
    let mut relevant_relations = vec![];
    for (i, entry) in relations.entries().enumerate() {
        let names = entry
            .relations()
            .map(|r| r.name().to_string())
            .collect::<Vec<_>>();
        if names.len() > 1 && names.contains(&package.to_string()) && is_obsolete(&entry) {
            obsolete_relations.push(i);
        }
        if names != [package] {
            continue;
        }
        found = true;
        if entry
            .relations()
            .next()
            .and_then(|r| r.version())
            .map(|(_vc, v)| &v < minimum_version)
            .unwrap_or(false)
        {
            relevant_relations.push(i);
        }
    }
    if !found {
        changed = true;
        relations.push(
            Relation::new(
                package,
                Some((VersionConstraint::GreaterThanEqual, minimum_version.clone())),
            )
            .into(),
        );
    } else {
        for i in relevant_relations.into_iter().rev() {
            relations.replace(
                i,
                Relation::new(
                    package,
                    Some((VersionConstraint::GreaterThanEqual, minimum_version.clone())),
                )
                .into(),
            );
            changed = true;
        }
    }
    for i in obsolete_relations.into_iter().rev() {
        relations.remove(i);
    }
    changed
}

/// Update a relation string to depend on a specific version.
///
/// # Arguments
/// * `relation` - Package relations
/// * `package` - Package name
/// * `version` - Exact version to depend on
/// * `position` - Optional position in the list to insert any new entries
pub fn ensure_exact_version(
    relations: &mut Relations,
    package: &str,
    version: &Version,
    position: Option<usize>,
) -> bool {
    let mut changed = false;
    let mut found = vec![];

    for (i, entry) in relations.entries().enumerate() {
        let names = entry
            .relations()
            .map(|r| r.name().to_string())
            .collect::<Vec<_>>();
        if names != [package] {
            continue;
        }
        let relation = entry.relations().next().unwrap();
        if relation.version().map_or(true, |(vc, v)| {
            vc != VersionConstraint::Equal || &v != version
        }) {
            found.push(i);
        }
    }
    if found.is_empty() {
        changed = true;
        let relation = Relation::new(package, Some((VersionConstraint::Equal, version.clone())));
        if let Some(position) = position {
            relations.insert(position, relation.into());
        } else {
            relations.push(relation.into());
        }
    } else {
        for i in found.into_iter().rev() {
            relations.replace(
                i,
                Relation::new(package, Some((VersionConstraint::Equal, version.clone()))).into(),
            );
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_control::lossless::relations::{Relation,Relations};

    mod is_dep_implied {
        use super::*;
        fn parse(s: &str) -> Relation {
            let rs: Relations = s.parse().unwrap();
            let mut entries = rs.entries();
            let entry = entries.next().unwrap();
            assert_eq!(entries.next(), None);
            let mut relations = entry.relations();
            let relation = relations.next().unwrap();
            assert_eq!(relations.next(), None);
            relation
        }

        fn is_dep_implied(inner: &str, outer: &str) -> bool {
            super::is_dep_implied(&parse(inner), &parse(outer))
        }

        #[test]
        fn test_no_version() {
            assert!(is_dep_implied("bzr", "bzr"));
            assert!(is_dep_implied("bzr", "bzr (>= 3)"));
            assert!(is_dep_implied("bzr", "bzr (<< 3)"));
        }

        #[test]
        fn test_wrong_package() {
            assert!(!is_dep_implied("bzr", "foo (<< 3)"));
        }

        #[test]
        fn test_version() {
            assert!(!is_dep_implied("bzr (>= 3)", "bzr (<< 3)"));
            assert!(is_dep_implied("bzr (>= 3)", "bzr (= 3)"));
            assert!(!is_dep_implied("bzr (= 3)", "bzr (>= 3)"));
            assert!(!is_dep_implied("bzr (>= 3)", "bzr (>> 3)"));
            assert!(!is_dep_implied("bzr (= 3)", "bzr (= 4)"));
            assert!(!is_dep_implied("bzr (>= 3)", "bzr (>= 2)"));
            assert!(is_dep_implied("bzr (>= 3)", "bzr (>= 3)"));
            assert!(is_dep_implied("bzr", "bzr (<< 3)"));
            assert!(is_dep_implied("bzr (<< 3)", "bzr (<< 3)"));
            assert!(is_dep_implied("bzr (<= 3)", "bzr (<< 3)"));
            assert!(!is_dep_implied("bzr (>= 2)", "bzr (<< 3)"));
            assert!(!is_dep_implied("bzr (<< 2)", "bzr (<< 3)"));
            assert!(!is_dep_implied("bzr (<= 2)", "bzr (<< 3)"));
            assert!(is_dep_implied("bzr (<= 5)", "bzr (<< 3)"));
            assert!(is_dep_implied("bzr (<= 5)", "bzr (= 3)"));
            assert!(!is_dep_implied("bzr (<= 5)", "bzr (>= 3)"));
            assert!(is_dep_implied("bzr (>> 5)", "bzr (>> 6)"));
            assert!(is_dep_implied("bzr (>> 5)", "bzr (>> 5)"));
            assert!(!is_dep_implied("bzr (>> 5)", "bzr (>> 4)"));
            assert!(is_dep_implied("bzr (>> 5)", "bzr (= 6)"));
            assert!(!is_dep_implied("bzr (>> 5)", "bzr (= 5)"));
            assert!(is_dep_implied("bzr:any (>> 5)", "bzr:any (= 6)"));
        }
    }

    mod is_relation_implied {
        use debian_control::lossless::relations::Relations;

        fn parse(s: &str) -> super::Entry {
            let r: Relations = s.parse().unwrap();
            let mut entries = r.entries();
            let entry = entries.next().unwrap();
            assert_eq!(entries.next(), None);
            entry
        }

        fn is_relation_implied(inner: &str, outer: &str) -> bool {
            super::is_relation_implied(&parse(inner), &parse(outer))
        }

        #[test]
        fn test_unrelated() {
            assert!(!is_relation_implied("bzr", "bar"));
            assert!(!is_relation_implied("bzr (= 3)", "bar"));
            assert!(!is_relation_implied("bzr (= 3) | foo", "bar"));
        }

        #[test]
        fn test_too_old() {
            assert!(!is_relation_implied("bzr (= 3)", "bzr"));
            assert!(!is_relation_implied("bzr (= 3)", "bzr (= 2)"));
            assert!(!is_relation_implied("bzr (= 3)", "bzr (>= 2)"));
        }

        #[test]
        fn test_ors() {
            assert!(!is_relation_implied("bzr (= 3)", "bzr | foo"));
            assert!(is_relation_implied("bzr", "bzr | foo"));
            assert!(is_relation_implied("bzr | foo", "bzr | foo"));
        }

        #[test]
        fn test_implied() {
            assert!(is_relation_implied("bzr (= 3)", "bzr (= 3)"));
            assert!(is_relation_implied("bzr (>= 3)", "bzr (>= 4)"));
            assert!(is_relation_implied("bzr (>= 4)", "bzr (>= 4)"));
            assert!(is_relation_implied("bzr", "bzr"));
            assert!(is_relation_implied("bzr | foo", "bzr"));
            assert!(!is_relation_implied("bzr (= 3)", "bzr (>= 3)"));
            assert!(is_relation_implied(
                "python3:any | dh-sequence-python3",
                "python3:any"
            ));
            assert!(is_relation_implied(
                "python3:any | python3-dev:any | dh-sequence-python3",
                "python3:any | python3-dev:any"
            ));
        }
    }

    #[test]
    fn test_ensure_relation() {
        let mut rels: Relations = "".parse().unwrap();
        let rel = Relation::simple("foo");
        ensure_relation(&mut rels, rel.into());
        assert_eq!("foo", rels.to_string());
    }

    #[test]
    fn test_ensure_relation_upgrade() {
        let mut rels = "foo".parse().unwrap();
        let newrel: Entry = Relation::new(
            "foo",
            Some((VersionConstraint::Equal, "1.0".parse().unwrap())),
        )
        .into();
        ensure_relation(&mut rels, newrel);
        assert_eq!("foo (= 1.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_relation_new() {
        let mut rels = "bar (= 1.0)".parse().unwrap();
        let newrel: Entry = Relation::new(
            "foo",
            Some((VersionConstraint::Equal, "2.0".parse().unwrap())),
        )
        .into();
        ensure_relation(&mut rels, newrel);
        assert_eq!("bar (= 1.0), foo (= 2.0)", rels.to_string());
    }

    #[test]
    fn test_drops_obsolete() {
        let mut rels = "bar (= 1.0), foo (>= 2.0), foo (>= 1.0)".parse().unwrap();
        let newrel: Entry = Relation::new(
            "foo",
            Some((VersionConstraint::GreaterThanEqual, "3.0".parse().unwrap())),
        )
        .into();
        ensure_relation(&mut rels, newrel);
        assert_eq!("bar (= 1.0), foo (>= 3.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_relation_with_error() {
        let mut rels = Relations::parse_relaxed("@cdbs@, debhelper (>= 9)", false).0;
        let newrel: Entry = Relation::new("foo", None).into();

        ensure_relation(&mut rels, newrel);
        assert_eq!("@cdbs@, debhelper (>= 9), foo", rels.to_string());
    }

    #[test]
    fn test_ensure_minimum_version() {
        let mut rels = "".parse().unwrap();
        ensure_minimum_version(&mut rels, "foo", &"1.0".parse().unwrap());
        assert_eq!("foo (>= 1.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_minimum_version_upgrade() {
        let mut rels = "foo (>= 1.0)".parse().unwrap();
        ensure_minimum_version(&mut rels, "foo", &"2.0".parse().unwrap());
        assert_eq!("foo (>= 2.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_minimum_version_upgrade_with_or() {
        let mut rels = "foo (>= 1.0) | bar".parse().unwrap();
        ensure_minimum_version(&mut rels, "foo", &"2.0".parse().unwrap());
        assert_eq!("foo (>= 2.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_exact_version() {
        let mut rels = "".parse().unwrap();
        ensure_exact_version(&mut rels, "foo", &"1.0".parse().unwrap(), None);
        assert_eq!("foo (= 1.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_exact_version_upgrade() {
        let mut rels = "foo (= 1.0)".parse().unwrap();
        ensure_exact_version(&mut rels, "foo", &"2.0".parse().unwrap(), None);
        assert_eq!("foo (= 2.0)", rels.to_string());
    }

    #[test]
    fn test_ensure_exact_version_upgrade_with_position() {
        let mut rels = "foo (= 1.0)".parse().unwrap();
        ensure_exact_version(&mut rels, "foo", &"2.0".parse().unwrap(), Some(0));
        assert_eq!("foo (= 2.0)", rels.to_string());
    }
}
