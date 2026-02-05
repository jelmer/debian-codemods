/// DEP-12 standard field order for upstream metadata
/// This defines the canonical ordering of fields in debian/upstream/metadata files
///
/// Based on the DEP-12 specification:
/// https://dep-team.pages.debian.net/deps/dep12/
pub const DEP12_FIELD_ORDER: &[&str] = &[
    "Name",
    "Contact",
    "Archive",
    "ASCL-Id",
    "Bug-Database",
    "Bug-Submit",
    "Changelog",
    "Cite-As",
    "CPE",
    "Documentation",
    "Donation",
    "FAQ",
    "Funding",
    "Gallery",
    "Other-References",
    "Reference",
    "Registration",
    "Registry",
    "Repository",
    "Repository-Browse",
    "Screenshots",
    "Security-Contact",
    "Webservice",
];
