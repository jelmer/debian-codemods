/// What workspace state a detector cares about.
///
/// LSP hosts use these to avoid running every detector on every keystroke:
/// when the user edits a file, only detectors whose triggers match the
/// changed location need to re-run. The lintian-brush CLI ignores
/// triggers and runs every registered detector unconditionally.
///
/// The lifetimes here are `'static` because triggers are declared at
/// build time via [`declare_detector!`] and stored in
/// [`DetectorRegistration`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Detector cares about a file at this exact workspace-relative path.
    File(&'static str),
    /// Detector cares about any file matching this glob (workspace-relative).
    /// Common cases: `"debian/*.service"`, `"debian/*.lintian-overrides"`.
    Glob(&'static str),
    /// Detector cares about a single deb822 field, named by `field`, in
    /// any paragraph that contains a key matching `paragraph_key`.
    ///
    /// `paragraph_key` is the name of an *identifying* field — its
    /// presence in a paragraph selects that paragraph as the trigger
    /// scope. For `debian/control`:
    /// * `paragraph_key = "Source"` selects the source paragraph.
    /// * `paragraph_key = "Package"` selects any binary paragraph.
    ///
    /// For `debian/copyright` (DEP-5 machine-readable):
    /// * `paragraph_key = "Format"` selects the header paragraph.
    /// * `paragraph_key = "Files"` selects any Files paragraph.
    /// * `paragraph_key = "License"` selects any standalone License
    ///   paragraph (Files paragraphs also carry `License:`, so a trigger
    ///   on `License` paragraphs only also matches Files paragraphs —
    ///   pair with a separate `Files` trigger if you want both).
    ///
    /// For `debian/tests/control`:
    /// * `paragraph_key = "Tests"` selects any paragraph identified by a
    ///   `Tests:` field (the common form).
    /// * `paragraph_key = "Test-Command"` selects any paragraph
    ///   identified by `Test-Command:` (an alternative form).
    ///
    /// Field names may end in a single `*` to match a prefix
    /// (`Vcs-*`); a bare `"*"` matches any field in the paragraph.
    Deb822Field {
        /// Workspace-relative path of the deb822 file.
        file: &'static str,
        /// Identifying field name selecting the paragraph kind.
        paragraph_key: &'static str,
        /// Field name; `*` is a prefix wildcard or full match-any.
        field: &'static str,
    },
    /// Detector cares about an aspect of `debian/watch`, expressed in
    /// terms of the watch-file conceptual model rather than the
    /// underlying syntax (line-based v1-4 vs deb822 v5). Hosts map this
    /// onto whichever syntactic form the package happens to use.
    Watch(WatchAspect),
    /// Detector cares about an aspect of `debian/changelog`. See
    /// [`ChangelogAspect`].
    Changelog(ChangelogAspect),
    /// Detector cares about a top-level field in `debian/upstream/metadata`
    /// (the YAML DEP-12 file).
    ///
    /// Field names follow the same wildcard rules as [`Deb822Field::field`]:
    /// a trailing `*` matches a prefix (e.g. `"Bug-*"` covers
    /// `Bug-Database` and `Bug-Submit`), and a bare `"*"` matches any
    /// top-level field.
    UpstreamMetadataField(&'static str),
    /// Detector cares about a specific key inside `debian/debcargo.toml`.
    ///
    /// `path` is a dot-separated TOML key path from the document root,
    /// e.g. `"source.homepage"` or `"source.vcs_git"`. A bare `"*"`
    /// matches any top-level key; a trailing `".*"` matches all keys
    /// within a table (e.g. `"source.*"`).
    DebcargoField(&'static str),
}

/// What a [`Trigger::Changelog`] detector reads from `debian/changelog`.
///
/// Modelled in terms of changelog entry parts rather than raw text, so
/// hosts can map the trigger to whichever part of an entry was edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangelogAspect {
    /// The version on any entry's header line.
    Version,
    /// The release distribution on any entry's header line (e.g.
    /// `unstable`, `UNRELEASED`).
    Distribution,
    /// The urgency on any entry's header line.
    Urgency,
    /// The body of any changelog entry — the asterisk-bullet items that
    /// describe what changed.
    Body,
    /// The maintainer name/email in any entry's trailer line.
    Maintainer,
    /// The date/time in any entry's trailer line.
    Timestamp,
}

/// What a [`Trigger::Watch`] detector reads from `debian/watch`.
///
/// The aspects are framed in terms of the watch-file model — a list of
/// upstream-source entries with options — independently of whether
/// they're encoded as line-based v1-4 syntax or v5 deb822 paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchAspect {
    /// The watch-file version declaration (line 1 of v1-4, the
    /// `Version:` field of the v5 header paragraph).
    Version,
    /// The source URL of any entry (any kind).
    Source,
    /// The matching-pattern of any non-template entry.
    MatchingPattern,
    /// Template entries of a given kind (e.g. `"GitHub"`, `"PyPI"`,
    /// `"CRAN"`); `"*"` matches any template kind.
    Template(&'static str),
    /// Entries that set a specific option (e.g. `"pgpsigurlmangle"`,
    /// `"dversionmangle"`, `"filenamemangle"`).
    Option(&'static str),
}
