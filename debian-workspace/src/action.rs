use debian_analyzer::Certainty;
use debian_control::relations::VersionConstraint;
use debversion::Version;
use std::path::{Path, PathBuf};

/// One self-consistent set of actions that fixes a [`Diagnostic`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionPlan {
    /// Imperative description of what this plan would do, shown to the
    /// user (LSP code-action menu, `lintian-brush --interactive`). Every
    /// plan must have one — a diagnostic with multiple plans needs each
    /// titled distinctly so the user can pick.
    pub label: String,
    /// If true, this plan only applies when the user has opted into
    /// opinionated fixes (`--opinionated` / `preferences.opinionated`).
    /// The driver skips opinionated plans otherwise.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub opinionated: bool,
    /// Confidence that this plan correctly addresses the diagnostic, as
    /// distinct from the diagnostic's own certainty that the issue exists.
    /// `None` means the plan makes no claim of its own; the driver treats
    /// it as [`Certainty::Certain`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certainty: Option<Certainty>,
    /// Actions applied as a unit.
    pub actions: Vec<Action>,
}

/// A change to apply to the working tree.
///
/// Dispatched on file kind: each per-file enum carries the actual operations.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// An edit to a deb822 file (debian/control, debian/copyright, …).
    Deb822(Deb822Action),
    /// An edit to a systemd unit file (.service, .socket, .target, …).
    Systemd(SystemdAction),
    /// An edit to a freedesktop .desktop entry file.
    DesktopIni(DesktopIniAction),
    /// An edit to a YAML file.
    Yaml(YamlAction),
    /// An edit to a `debian/changelog` file.
    Changelog(ChangelogAction),
    /// An edit to a `debian/watch` file.
    Watch(WatchAction),
    /// An edit to a Makefile (typically `debian/rules`).
    Makefile(MakefileAction),
    /// An edit to a DEP-3 patch header (a quilt patch under
    /// `debian/patches/`).
    Dep3(Dep3Action),
    /// An edit to a lintian-overrides file (`debian/source/lintian-overrides`
    /// or `debian/<pkg>.lintian-overrides`).
    LintianOverrides(LintianOverridesAction),
    /// An edit to a maintscript file (`debian/maintscript` or
    /// `debian/<pkg>.maintscript`).
    Maintscript(MaintscriptAction),
    /// An edit to a `debian/debcargo.toml` file. Used for Rust crate
    /// packages where the control file is generated.
    Debcargo(DebcargoAction),
    /// Invoke an external tool that mutates files in the working tree (e.g.
    /// `debconf-updatepo`). Use this only when the operation can't be
    /// expressed as one of the typed file actions above.
    RunCommand(RunCommandAction),
    /// A filesystem-level edit (chmod, write, delete, byte-range replace).
    Filesystem(FilesystemAction),
}

/// Continuation-line indent pattern for multi-line deb822 field values.
///
/// Mirrors [`deb822_lossless::IndentPattern`] but is `serde`-serialisable
/// so it can travel over the LSP wire alongside actions.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndentPattern {
    /// All continuation lines use exactly `spaces` leading spaces.
    Fixed {
        /// Number of leading spaces (typically `1` for DEP-5 / Description).
        spaces: usize,
    },
    /// Continuation lines align with the column after the field name and
    /// `": "`, i.e. `field_name.len() + 2` spaces. The deb822 default for
    /// most fields.
    FieldNameLength,
}

impl IndentPattern {
    /// Convert to the underlying `deb822_lossless` pattern for the
    /// applier.
    pub fn to_deb822(&self) -> deb822_lossless::IndentPattern {
        match self {
            IndentPattern::Fixed { spaces } => deb822_lossless::IndentPattern::Fixed(*spaces),
            IndentPattern::FieldNameLength => deb822_lossless::IndentPattern::FieldNameLength,
        }
    }
}

/// The file an action targets, for grouping actions by file before they are
/// applied.
///
/// `Rename` returns its *source* path here and `RunCommand` returns its
/// monitored scope directory; neither is the full set of paths the action
/// modifies. The authoritative modified set is what
/// [`apply_actions`](crate::appliers::apply_actions) returns after running
/// the appliers: the appliers observe what actually changed (a `Rename`
/// touches both endpoints, a `RunCommand` touches whatever its command
/// wrote).
pub(crate) fn action_file(action: &Action) -> &Path {
    match action {
        Action::Deb822(a) => match a {
            Deb822Action::SetField { file, .. }
            | Deb822Action::SetFieldWithIndent { file, .. }
            | Deb822Action::RemoveField { file, .. }
            | Deb822Action::RenameField { file, .. }
            | Deb822Action::RemoveParagraph { file, .. }
            | Deb822Action::AppendParagraph { file, .. }
            | Deb822Action::NormalizeFieldSpacing { file, .. }
            | Deb822Action::DropRelation { file, .. }
            | Deb822Action::DropRelationEntry { file, .. }
            | Deb822Action::ReplaceRelation { file, .. }
            | Deb822Action::SetRelationVersionConstraint { file, .. }
            | Deb822Action::EnsureSubstvar { file, .. }
            | Deb822Action::DropSubstvar { file, .. }
            | Deb822Action::EnsureRelation { file, .. }
            | Deb822Action::MoveRelation { file, .. }
            | Deb822Action::MakeAlternativePrimary { file, .. }
            | Deb822Action::AddAlternative { file, .. }
            | Deb822Action::ReorderParagraphs { file, .. }
            | Deb822Action::DropFieldComments { file, .. } => file,
        },
        Action::Systemd(a) => match a {
            SystemdAction::SetField { file, .. }
            | SystemdAction::RemoveField { file, .. }
            | SystemdAction::RenameField { file, .. }
            | SystemdAction::Add { file, .. }
            | SystemdAction::RemoveValue { file, .. } => file,
        },
        Action::DesktopIni(a) => match a {
            DesktopIniAction::SetField { file, .. }
            | DesktopIniAction::RemoveField { file, .. }
            | DesktopIniAction::RemoveAll { file, .. }
            | DesktopIniAction::RenameField { file, .. } => file,
        },
        Action::Yaml(a) => match a {
            YamlAction::SetField { file, .. }
            | YamlAction::SetFieldOrdered { file, .. }
            | YamlAction::RemoveField { file, .. }
            | YamlAction::RenameField { file, .. } => file,
        },
        Action::Changelog(a) => match a {
            ChangelogAction::ReplaceEntryChanges { file, .. }
            | ChangelogAction::SetEntryDate { file, .. }
            | ChangelogAction::RemoveBullet { file, .. }
            | ChangelogAction::ReplaceBullet { file, .. }
            | ChangelogAction::SetEntryVersion { file, .. } => file,
        },
        Action::Watch(a) => match a {
            WatchAction::SetEntryMatchingPattern { file, .. }
            | WatchAction::RemoveEntryOption { file, .. }
            | WatchAction::SetEntryOption { file, .. }
            | WatchAction::SetEntryUrl { file, .. }
            | WatchAction::ConvertEntryToTemplate { file, .. } => file,
        },
        Action::Makefile(a) => match a {
            MakefileAction::ReplaceRecipe { file, .. }
            | MakefileAction::RemoveRecipe { file, .. }
            | MakefileAction::SetVariable { file, .. }
            | MakefileAction::SetVariableOperator { file, .. }
            | MakefileAction::RemoveVariable { file, .. }
            | MakefileAction::RemoveRule { file, .. }
            | MakefileAction::RemovePhonyTarget { file, .. }
            | MakefileAction::RenameRuleTarget { file, .. }
            | MakefileAction::AddRule { file, .. }
            | MakefileAction::AddPhonyTarget { file, .. }
            | MakefileAction::AddInclude { file, .. }
            | MakefileAction::ReplaceVariableWithInclude { file, .. }
            | MakefileAction::InsertIncludeBeforeVariable { file, .. } => file,
        },
        Action::Dep3(a) => match a {
            Dep3Action::SetField { file, .. }
            | Dep3Action::RemoveField { file, .. }
            | Dep3Action::RenameField { file, .. } => file,
        },
        Action::LintianOverrides(a) => match a {
            LintianOverridesAction::AddLine { file, .. }
            | LintianOverridesAction::DropLine { file, .. }
            | LintianOverridesAction::RenameTag { file, .. }
            | LintianOverridesAction::SetLineInfo { file, .. } => file,
        },
        Action::Maintscript(a) => match a {
            MaintscriptAction::DropEntry { file, .. } => file,
        },
        Action::Debcargo(a) => match a {
            DebcargoAction::SetSourceField { file, .. }
            | DebcargoAction::SetTopLevelBool { file, .. } => file,
        },
        Action::RunCommand(a) => match a {
            RunCommandAction::Run { scope, .. } => scope,
        },
        Action::Filesystem(a) => match a {
            FilesystemAction::SetMode { file, .. }
            | FilesystemAction::Delete { file }
            | FilesystemAction::Rename { file, .. }
            | FilesystemAction::RemoveDirIfEmpty { file }
            | FilesystemAction::Write { file, .. }
            | FilesystemAction::ReplaceText { file, .. }
            | FilesystemAction::Substitute { file, .. }
            | FilesystemAction::NormalizeLineEndings { file } => file,
        },
    }
}

/// Edits to a deb822 file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Deb822Action {
    /// Set a field value, inserting it if missing.
    ///
    /// Continuation-line indentation for multi-line values follows the
    /// deb822 default: align continuations to the field-name column. Use
    /// [`SetFieldWithIndent`](Self::SetFieldWithIndent) when a field
    /// needs a specific indent (e.g. Description / DEP-5 mandate a
    /// single-space indent).
    SetField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Field name.
        field: String,
        /// New value.
        value: String,
    },
    /// Like [`SetField`](Self::SetField), but with an explicit
    /// continuation-line indent pattern. Used for fields whose
    /// formatting convention diverges from the deb822 default — most
    /// notably binary-package `Description:` (single-space indent per
    /// DEP-5) and debian/copyright bodies.
    SetFieldWithIndent {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Field name.
        field: String,
        /// New value.
        value: String,
        /// Continuation-line indent pattern.
        indent: IndentPattern,
    },
    /// Remove a field if present.
    RemoveField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Field name.
        field: String,
    },
    /// Rename a field, preserving its value.
    RenameField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Current field name.
        from: String,
        /// New field name.
        to: String,
    },
    /// Remove the paragraph identified by `paragraph`.
    RemoveParagraph {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to drop.
        paragraph: ParagraphSelector,
    },
    /// Append a new paragraph at the end of the file with the given
    /// (field, value) pairs in order.
    AppendParagraph {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Fields to populate the new paragraph with.
        fields: Vec<(String, String)>,
        /// Continuation-line indent for multi-line values, in spaces.
        /// `None` lets the deb822 renderer auto-align to the field-name
        /// column (the default for debian/control). Use `Some(1)` for
        /// debian/copyright, where DEP-5 mandates a single-space indent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indent: Option<usize>,
    },
    /// Normalize the whitespace around a field's separator (`:` and the
    /// continuation indent). The deb822 spec allows arbitrary spacing
    /// after the colon, but the convention is exactly one space; this
    /// action collapses unusual spacing without otherwise touching the
    /// value. A no-op if the field already has canonical spacing.
    NormalizeFieldSpacing {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Field name.
        field: String,
    },
    /// Drop every relation matching `package` from a relations field
    /// (Depends, Build-Depends, etc.). Empty alternative groups are
    /// removed; if the field becomes empty it is removed entirely. A
    /// no-op if the package isn't named in the field.
    DropRelation {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Build-Depends`).
        field: String,
        /// Package name to drop.
        package: String,
    },
    /// Drop the alternative entry in a relations field whose parsed value
    /// equals `entry` (e.g. `libfoo-perl | perl`). Unlike
    /// [`DropRelation`](Self::DropRelation), which only removes entries that
    /// name a single package, this targets a whole alternative group by its
    /// text. If the field becomes empty it is removed entirely. A no-op if no
    /// entry matches.
    DropRelationEntry {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Depends`).
        field: String,
        /// Entry text to drop (e.g. `libfoo-perl | perl`).
        entry: String,
    },
    /// Replace the first relation that names `from_package` with the
    /// `to_entry` text, keeping the entry's position in the field. A
    /// no-op if `from_package` isn't named. If `to_entry` parses as a
    /// relation whose package is already named elsewhere in the field,
    /// the original `from_package` entry is dropped without inserting a
    /// duplicate.
    ReplaceRelation {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Build-Depends`).
        field: String,
        /// Package name (matched exactly) of the relation to replace.
        from_package: String,
        /// New entry text (e.g. `perl`, `debhelper (>= 12)`).
        to_entry: String,
    },
    /// Ensure a substvar (`${...}`) is present in a relations field. If
    /// the field doesn't exist it's created with just the substvar; if
    /// it exists and already mentions the substvar it's a no-op.
    EnsureSubstvar {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Depends`).
        field: String,
        /// Substvar to ensure, including the surrounding `${...}`.
        substvar: String,
    },
    /// Drop a substvar (`${...}`) from a relations field. If the field
    /// becomes empty it's removed entirely. A no-op if the substvar is
    /// already absent.
    DropSubstvar {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name.
        field: String,
        /// Substvar to drop, including the surrounding `${...}`.
        substvar: String,
    },
    /// Ensure a relation entry is present in a relations field, creating
    /// the field if necessary. `entry` is a literal relation entry string
    /// (e.g. `python3-poetry-core` or `debhelper-compat (= 13)`).
    ///
    /// If `entry` carries no version constraint the action is a no-op
    /// when any relation with the same package name is already present.
    /// If `entry` has an exact version, the action upgrades any existing
    /// relation to that exact version.
    EnsureRelation {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Build-Depends`).
        field: String,
        /// Literal relation entry to ensure.
        entry: String,
    },
    /// Set the version constraint on every relation in `field` that names
    /// `package`. Acts per-relation, so the constraint is replaced without
    /// removing the package from the field or affecting any alternatives in
    /// the same entry. Passing `None` drops the constraint entirely. A no-op
    /// if the package isn't named in `field` or every matching relation
    /// already has the requested constraint.
    SetRelationVersionConstraint {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Depends`).
        field: String,
        /// Package name to set the version constraint on.
        package: String,
        /// New constraint, or `None` to strip the constraint entirely.
        constraint: Option<(VersionConstraint, Version)>,
    },
    /// Move a relation entry between two fields of the same paragraph,
    /// preserving its version constraint and any alternatives. The entry
    /// is identified by `package`. If `from_field` becomes empty after
    /// the move it is removed entirely. A no-op if the package isn't
    /// present in `from_field`.
    MoveRelation {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Source relations field name.
        from_field: String,
        /// Destination relations field name.
        to_field: String,
        /// Package name identifying the entry to move.
        package: String,
    },
    /// Reorder the alternatives in the relation entry that names
    /// `package` so that `package` becomes the primary (first)
    /// alternative. The other alternatives keep their relative order;
    /// each alternative's version and architecture qualifiers are
    /// preserved verbatim, and the `|` separators are normalised to the
    /// conventional ` | `.
    ///
    /// Operates on the first entry of `field` that names `package`. A
    /// no-op if `package` isn't named in `field`, or already heads its
    /// entry.
    MakeAlternativePrimary {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Depends`).
        field: String,
        /// Package name whose alternative should become primary.
        package: String,
    },
    /// Append an alternative to the relation entry that names `package`.
    ///
    /// Operates on the first entry of `field` that names `package`. The
    /// existing alternatives keep their order and qualifiers; `alternative`
    /// (a literal relation, e.g. `mail-transport-agent`) is added after
    /// them, joined with the conventional ` | `. A no-op if `package`
    /// isn't named in `field`, or the entry already lists `alternative`.
    AddAlternative {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Relations field name (e.g. `Depends`).
        field: String,
        /// Package name identifying the entry to extend.
        package: String,
        /// Literal relation to add as a trailing alternative.
        alternative: String,
    },
    /// Reorder a subset of paragraphs in a deb822 file. Paragraphs that
    /// have `key_field` are pulled out and re-inserted in the order
    /// given by `order` (which lists their `key_field` values). Other
    /// paragraphs stay in place: the i-th slot occupied by a
    /// participating paragraph in the original document is filled by
    /// the i-th key from `order`. Keys in `order` that aren't present
    /// in the document are skipped.
    ReorderParagraphs {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Field whose presence marks a paragraph as participating in
        /// the reorder, and whose value identifies it.
        key_field: String,
        /// Desired order of `key_field` values among the participating
        /// paragraphs.
        order: Vec<String>,
    },
    /// Drop the commented-out lines embedded in a field's value.
    ///
    /// A deb822 field's value can be followed by `#`-prefixed lines that
    /// the parser keeps attached to that field — e.g. the commented-out
    /// `Vcs-*` lines old `dh_make` versions append after `Homepage`.
    /// This rewrites the field to its comment-free value, dropping those
    /// lines. A no-op if the field carries no embedded comment lines.
    DropFieldComments {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which paragraph to edit.
        paragraph: ParagraphSelector,
        /// Field name.
        field: String,
    },
}

/// Edits to a systemd unit file.
///
/// Systemd unit files are sectioned ini-style files (`[Unit]`, `[Service]`,
/// `[Install]`, …). Each variant identifies a single section by name and
/// targets one entry within it.
///
/// Multi-valued fields (e.g. `Alias=`, `After=`) are handled by
/// [`Add`](Self::Add) / [`RemoveValue`](Self::RemoveValue) — these append a
/// new value or remove a specific one without touching siblings.
/// [`SetField`](Self::SetField) replaces every occurrence of the key with a
/// single value, which is the right thing for scalar fields like `PIDFile=`
/// but the wrong thing for multi-valued ones.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SystemdAction {
    /// Set a scalar field. Replaces every existing entry with the given key.
    SetField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Section name, e.g. "Service".
        section: String,
        /// Field name (no trailing `=`).
        field: String,
        /// New value.
        value: String,
    },
    /// Remove every entry with the given key.
    RemoveField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Section name.
        section: String,
        /// Field name.
        field: String,
    },
    /// Rename every entry with `from` to `to`, preserving values.
    RenameField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Section name.
        section: String,
        /// Current field name.
        from: String,
        /// New field name.
        to: String,
    },
    /// Append a new entry. Use for multi-valued fields like `After=` or
    /// `Alias=` to add another value without disturbing siblings.
    Add {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Section name.
        section: String,
        /// Field name.
        field: String,
        /// Value to append.
        value: String,
    },
    /// Remove a specific value from a multi-valued field. Other values for
    /// the same key are preserved.
    RemoveValue {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Section name.
        section: String,
        /// Field name.
        field: String,
        /// Value to drop.
        value: String,
    },
}

/// Edits to a freedesktop `.desktop` entry file.
///
/// Desktop entry files are sectioned ini-style files with `[Group]`
/// headers and locale-tagged keys (e.g. `Name[de]=...`). Each variant
/// identifies one group and one entry within it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DesktopIniAction {
    /// Set a key. If `locale` is `None`, sets the unlocalised entry;
    /// otherwise sets the entry tagged with `locale` (e.g. `de`).
    SetField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Group name, e.g. "Desktop Entry".
        group: String,
        /// Key name.
        field: String,
        /// Locale tag, if any.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        locale: Option<String>,
        /// New value.
        value: String,
    },
    /// Remove a key. If `locale` is `None`, removes the unlocalised entry
    /// only; if a locale is given, removes only that locale variant. To
    /// drop every locale variant of a key, use [`RemoveAll`](Self::RemoveAll).
    RemoveField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Group name.
        group: String,
        /// Key name.
        field: String,
        /// Locale tag, if any.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        locale: Option<String>,
    },
    /// Remove a key together with every locale variant.
    RemoveAll {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Group name.
        group: String,
        /// Key name.
        field: String,
    },
    /// Rename a key, preserving its value (and every locale variant).
    RenameField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Group name.
        group: String,
        /// Current key name.
        from: String,
        /// New key name.
        to: String,
    },
}

/// Edits to a YAML file.
///
/// A YAML file is a tree of mappings, sequences and scalars; the
/// `parent_path` field navigates from the top-level document down to the
/// mapping that owns the key being edited. An empty `parent_path` means
/// the top-level mapping (the common case for Debian's flat YAML files
/// like `debian/upstream/metadata`).
///
/// Each path component is either a string (mapping key) or an index
/// (sequence position).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum YamlAction {
    /// Set a scalar value at `parent_path`'s mapping under `key`. Inserts
    /// the key if missing. New keys are appended at the end of the
    /// mapping; use [`SetFieldOrdered`](Self::SetFieldOrdered) to
    /// position the new key according to a canonical field order
    /// (e.g. DEP-12).
    SetField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path from the document root to the parent mapping.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parent_path: Vec<YamlPathComponent>,
        /// Key to set (string scalar).
        key: String,
        /// New value (string scalar).
        value: String,
    },
    /// Like [`SetField`](Self::SetField), but when inserting a new key,
    /// position it according to `field_order`. Keys not listed in
    /// `field_order` are placed at the end. A no-op if the key already
    /// exists with the requested value.
    SetFieldOrdered {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path from the document root to the parent mapping.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parent_path: Vec<YamlPathComponent>,
        /// Key to set (string scalar).
        key: String,
        /// New value (string scalar).
        value: String,
        /// Canonical field order. Keys appearing earlier in this list
        /// are placed earlier in the mapping.
        field_order: Vec<String>,
    },
    /// Remove a key from the mapping at `parent_path`.
    RemoveField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path from the document root to the parent mapping.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parent_path: Vec<YamlPathComponent>,
        /// Key to remove.
        key: String,
    },
    /// Rename a key in the mapping at `parent_path`, preserving its
    /// value and position.
    RenameField {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path from the document root to the parent mapping.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parent_path: Vec<YamlPathComponent>,
        /// Current key name.
        from: String,
        /// New key name.
        to: String,
    },
}

/// One step in a [`YamlAction`] path.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum YamlPathComponent {
    /// A mapping key (string).
    Key {
        /// Key name.
        key: String,
    },
    /// A sequence index (0-based).
    Index {
        /// Position.
        index: usize,
    },
}

/// Edits to a `debian/changelog`.
///
/// Operations target entries by their version, which is stable across
/// minor edits. Change-line content is supplied verbatim — the applier
/// preserves the changelog's existing indentation rules when re-rendering.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ChangelogAction {
    /// Replace the change lines of the entry with the given version. The
    /// `lines` are stored verbatim with their leading `  *`/`    `
    /// continuation prefix; the applier writes them as-is into the entry.
    ReplaceEntryChanges {
        /// File to edit, relative to the package root. Almost always
        /// `debian/changelog`, but kept explicit for symmetry.
        file: PathBuf,
        /// Version string of the target entry (e.g. `2.6.0-1`).
        version: String,
        /// Replacement change lines (one per line, no trailing newline).
        lines: Vec<String>,
    },
    /// Set the trailer datetime of the entry with the given version.
    ///
    /// The datetime is stored as an RFC 2822 string (`"Sun, 22 Apr 2018
    /// 00:58:14 +0000"`) — what `chrono::DateTime::to_rfc2822` produces
    /// and what changelog trailers use natively.
    SetEntryDate {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Version string of the target entry.
        version: String,
        /// New datetime as an RFC 2822 string.
        rfc2822: String,
    },
    /// Remove a bullet from the entry with the given version.
    ///
    /// The bullet is identified by its author attribution (the `[ Name ]`
    /// header that introduces multi-author groups, or `None` for an entry
    /// without one) and its body text (the bullet's lines joined with
    /// `\n`, exactly as `debian_changelog`'s `Bullet::lines()` returns
    /// them).
    ///
    /// `occurrence` is a 0-based index that disambiguates when several
    /// bullets share the same `(author, text)` key: `0` removes the first
    /// match, `1` the second, etc. The applier walks bullets in
    /// `iter_changes_by_author` order. Whitespace between surviving
    /// bullets is preserved.
    RemoveBullet {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Version string of the target entry.
        version: String,
        /// Author header above the bullet, if any.
        author: Option<String>,
        /// Body text of the bullet (lines joined by `\n`).
        text: String,
        /// 0-based index among bullets sharing the same `(author, text)`
        /// key. Defaults to `0` over the wire when omitted.
        #[serde(default)]
        occurrence: usize,
    },
    /// Replace the body lines of a bullet, identified the same way as in
    /// [`RemoveBullet`](Self::RemoveBullet). `new_lines` are stored
    /// without their `  *`/`    ` continuation prefix — the applier
    /// passes them straight to `Bullet::replace_with`, which re-adds the
    /// proper indentation.
    ReplaceBullet {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Version string of the target entry.
        version: String,
        /// Author header above the bullet, if any.
        author: Option<String>,
        /// Current body text of the bullet (lines joined by `\n`).
        text: String,
        /// 0-based index among bullets sharing the same `(author, text)`
        /// key.
        #[serde(default)]
        occurrence: usize,
        /// Replacement body lines.
        new_lines: Vec<String>,
    },
    /// Replace the version of the entry currently identified by `version`
    /// with `new_version`. A no-op if no entry has that version.
    SetEntryVersion {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Current version string of the target entry.
        version: String,
        /// New version string to write into the entry header.
        new_version: String,
    },
}
/// Edits to a `debian/watch` file.
///
/// Watch files are line-oriented, with each non-comment line describing a
/// release-monitor entry: a URL, a matching regexp for the version, and
/// optional `opts=...` flags. We address an entry by its current URL,
/// which is unique across the watch files we routinely fix.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WatchAction {
    /// Replace the matching pattern (the regexp following the URL) of the
    /// entry whose current URL is `url`. A no-op if no entry matches.
    SetEntryMatchingPattern {
        /// File to edit, relative to the package root. Almost always
        /// `debian/watch`.
        file: PathBuf,
        /// Current URL of the target entry.
        url: String,
        /// New matching pattern.
        new_pattern: String,
    },
    /// Remove an `opts=...` option from the entry whose current URL is
    /// `url`. A no-op if no entry matches or the option isn't set.
    RemoveEntryOption {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Current URL of the target entry.
        url: String,
        /// Name of the option to remove (e.g. `filenamemangle`).
        option: String,
    },
    /// Set (or insert) an `opts=...` option on the entry whose current URL
    /// is `url`. A no-op if no entry matches or the option already has the
    /// requested value.
    SetEntryOption {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Current URL of the target entry.
        url: String,
        /// Name of the option to set (e.g. `dversionmangle`).
        option: String,
        /// New value for the option.
        value: String,
    },
    /// Replace the URL of the entry whose current URL is `url`. A no-op if
    /// no entry matches.
    SetEntryUrl {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Current URL of the target entry.
        url: String,
        /// New URL.
        new_url: String,
    },
    /// Convert the v5 entry whose current URL is `url` to its template
    /// form (Template:/Owner:/Project: for GitHub, Template:/Dist: for
    /// CPAN/PyPI, etc.). A no-op if the entry is already a template,
    /// no template matches the URL/pattern, or no entry has that URL.
    ConvertEntryToTemplate {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Current URL of the target entry.
        url: String,
    },
}

/// Edits to a Makefile (typically `debian/rules`).
///
/// Recipes are addressed by their exact current text (including leading
/// indentation). This avoids index drift when multiple recipe edits target
/// the same rule.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MakefileAction {
    /// Replace the first recipe whose text exactly matches `recipe` in the
    /// rule whose primary target is `target`. A no-op if no rule or recipe
    /// matches.
    ReplaceRecipe {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Primary target of the rule containing the recipe.
        target: String,
        /// Current recipe text, matched verbatim (including indentation).
        recipe: String,
        /// Replacement recipe text. The applier preserves the original
        /// recipe's leading whitespace if `new_recipe` doesn't start with
        /// whitespace itself.
        new_recipe: String,
    },
    /// Remove the first recipe whose text exactly matches `recipe` from the
    /// rule whose primary target is `target`. A no-op if no rule or recipe
    /// matches.
    RemoveRecipe {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Primary target of the rule containing the recipe.
        target: String,
        /// Recipe text, matched verbatim (including indentation).
        recipe: String,
    },
    /// Replace the value of the first variable definition for `name`.
    /// A no-op if no such variable exists.
    SetVariable {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Variable name (matched exactly).
        name: String,
        /// New right-hand side, verbatim (no quoting applied).
        value: String,
    },
    /// Change the assignment operator on the first variable definition
    /// for `name` (e.g. `:=` to `?=`). A no-op if no such variable
    /// exists or it already uses `operator`.
    SetVariableOperator {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Variable name (matched exactly).
        name: String,
        /// New assignment operator (`=`, `:=`, `?=`, `+=`).
        operator: String,
    },
    /// Remove the first variable definition for `name`. A no-op if no such
    /// variable exists.
    RemoveVariable {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Variable name (matched exactly).
        name: String,
    },
    /// Remove the first rule whose primary target is `target`. A no-op if
    /// no such rule exists.
    RemoveRule {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Primary target of the rule to remove.
        target: String,
    },
    /// Remove `target` from the prerequisites of the `.PHONY` rule. If
    /// `.PHONY` becomes empty, the rule itself is removed. A no-op if
    /// the target is not listed.
    RemovePhonyTarget {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Target name to remove from `.PHONY`.
        target: String,
    },
    /// Rename a target on the first rule that has it. A no-op if no rule
    /// has the old target.
    RenameRuleTarget {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Old target name (matched exactly after trimming).
        from_target: String,
        /// New target name.
        to_target: String,
    },
    /// Append a new rule with `target` and the given (possibly empty)
    /// prerequisites. The applier does not check for an existing rule —
    /// detectors must guard against duplicates themselves.
    AddRule {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Target name for the new rule.
        target: String,
        /// Prerequisite targets (in order).
        prerequisites: Vec<String>,
    },
    /// Add `target` to the prerequisites of the `.PHONY` rule. A no-op if
    /// `.PHONY` already lists `target`. If no `.PHONY` rule exists, the
    /// applier creates one.
    AddPhonyTarget {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Target name to add to `.PHONY`.
        target: String,
    },
    /// Add an `include <path>` directive. A no-op if the file is already
    /// included.
    AddInclude {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path to include (e.g. `/usr/share/dpkg/pkg-info.mk`).
        path: String,
    },
    /// Replace the first variable definition for `name` with an
    /// `include <path>` directive. A no-op if the variable doesn't
    /// exist or `path` is already included. Used to migrate
    /// `DEB_HOST_ARCH := $(shell dpkg-architecture -qDEB_HOST_ARCH)` and
    /// friends to a single `include /usr/share/dpkg/architecture.mk`,
    /// keeping the include in the variable's old position.
    ReplaceVariableWithInclude {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Variable name to replace (matched exactly).
        name: String,
        /// Path to include in place of the variable.
        path: String,
    },
    /// Insert `include <path>` immediately before the first variable
    /// definition whose name is `before_variable`. A no-op if the
    /// variable doesn't exist or `path` is already included.
    InsertIncludeBeforeVariable {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Path to include.
        path: String,
        /// Variable name to anchor the insertion against.
        before_variable: String,
    },
}

/// Edits to a DEP-3 patch header.
///
/// DEP-3 headers live at the top of a quilt patch (under
/// `debian/patches/`) followed by a blank line and the unified diff. The
/// applier parses just the header (everything before the first `---`,
/// `diff `, or `Index:` line), edits it, and reassembles the file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Dep3Action {
    /// Set a field's value, inserting it if missing. The field is added in
    /// the patch header's existing position when present, or appended.
    SetField {
        /// Patch file to edit, relative to the package root (e.g.
        /// `debian/patches/foo.patch`).
        file: PathBuf,
        /// Field name (case-sensitive, e.g. `Author`).
        field: String,
        /// New value.
        value: String,
    },
    /// Remove a field. A no-op if the field isn't present.
    RemoveField {
        /// Patch file to edit, relative to the package root.
        file: PathBuf,
        /// Field name to remove.
        field: String,
    },
    /// Rename `from_field` to `to_field`, preserving its value. A no-op
    /// if `from_field` isn't present. If `to_field` already exists, it is
    /// overwritten.
    RenameField {
        /// Patch file to edit, relative to the package root.
        file: PathBuf,
        /// Current field name.
        from_field: String,
        /// New field name.
        to_field: String,
    },
}

/// Identifies a specific override line for in-place edits.
///
/// We address lines by their visible content rather than by index because
/// other actions may shift the line numbering between detect and apply.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OverrideLineSelector {
    /// Tag name (matched exactly).
    pub tag: String,
    /// Optional info string the override carries (matched exactly, no
    /// wildcard expansion). `None` matches lines with no info.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    /// Optional package name from the `package:` prefix. `None` matches
    /// lines without a package spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// Edits to a `debian/source/lintian-overrides` or
/// `debian/<pkg>.lintian-overrides` file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LintianOverridesAction {
    /// Append a new override line. If the file does not exist it is
    /// created (including any missing parent directories). The line is
    /// only added when no existing line already overrides the same tag
    /// (same tag + same optional info), so the action is idempotent.
    AddLine {
        /// File to edit, relative to the package root (e.g.
        /// `debian/source/lintian-overrides` or
        /// `debian/mypkg.lintian-overrides`).
        file: PathBuf,
        /// Optional package name prefix (e.g. `mypkg`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        /// Tag name.
        tag: String,
        /// Optional info string.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        info: Option<String>,
    },
    /// Drop the first override line that matches `selector`. Each
    /// DropLine action consumes one line — to remove N copies of the
    /// same line, emit N actions. If the file becomes empty (no
    /// override lines remain), it is removed entirely.
    DropLine {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which override line to drop.
        selector: OverrideLineSelector,
    },
    /// Rename the tag on every line whose current tag is `from_tag`. The
    /// rest of each line (whitespace, comments, package spec, info) is
    /// preserved verbatim.
    RenameTag {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Old tag name (matched exactly).
        from_tag: String,
        /// New tag name.
        to_tag: String,
    },
    /// Rewrite the info text on the first line that matches `selector`.
    /// Only the info portion changes — the package spec, tag, and
    /// surrounding whitespace are preserved.
    SetLineInfo {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Which override line to update.
        selector: OverrideLineSelector,
        /// New info text. Empty string removes the info entirely.
        new_info: String,
    },
}

/// Edits to a maintscript file.
///
/// Each line in a maintscript file is an independent dpkg-maintscript-helper
/// invocation. We address entries by their exact text, mirroring how
/// [`MakefileAction::ReplaceRecipe`] addresses recipe lines.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MaintscriptAction {
    /// Drop the first entry whose trimmed line text equals `entry`.
    /// Comments immediately preceding the dropped line are also removed.
    /// If the file ends up empty (no entries remain), it is removed
    /// entirely. Each `DropEntry` consumes one matching line — to remove
    /// N copies of the same entry, emit N actions.
    DropEntry {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Entry text to drop, matched after trimming surrounding
        /// whitespace.
        entry: String,
    },
}

/// Edits to a `debian/debcargo.toml` file.
///
/// Debcargo manages its own control file; we manipulate scalar fields under
/// the `[source]` table directly. Only a small set of operations is needed
/// in practice — the equivalent of typed setters on the generated control
/// fields (Vcs-Git, Vcs-Browser, Standards-Version, Section).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DebcargoAction {
    /// Set a string field on the `[source]` table. Creates the table and/or
    /// the field if absent. Overwrites any existing value.
    SetSourceField {
        /// File to edit, relative to the package root. Almost always
        /// `debian/debcargo.toml`.
        file: PathBuf,
        /// Key inside `[source]` (e.g. `vcs_git`, `vcs_browser`,
        /// `section`, `standards_version`).
        field: String,
        /// New string value.
        value: String,
    },
    /// Set a boolean field at the top level of the file. Creates the field if
    /// absent. Overwrites any existing value.
    SetTopLevelBool {
        /// File to edit, relative to the package root. Almost always
        /// `debian/debcargo.toml`.
        file: PathBuf,
        /// Top-level key (e.g. `collapse_features`).
        field: String,
        /// New boolean value.
        value: bool,
    },
}

/// Run an external command that mutates the working tree.
///
/// Use sparingly: prefer typed actions whenever possible. The intended
/// use is tools like `debconf-updatepo` that produce changes we can't
/// describe declaratively.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RunCommandAction {
    /// Run `argv` in the package root. The applier snapshots `scope`
    /// before and after the run, considering the action a change iff any
    /// file under `scope` was added, removed, or had its bytes change.
    /// A non-zero exit code is a fixer error. ENOENT on `argv[0]` is
    /// reported as [`FixerError::MissingDependency`].
    Run {
        /// Argument vector. `argv[0]` is resolved via PATH.
        argv: Vec<String>,
        /// Subtree to monitor for changes, relative to the package root.
        /// Use `.` to monitor the entire tree.
        scope: PathBuf,
        /// Environment overrides applied on top of the inherited env.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env: Vec<(String, String)>,
    },
}

/// Filesystem-level edits.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FilesystemAction {
    /// Set the file mode (e.g. mark a script executable).
    SetMode {
        /// File to chmod, relative to the package root.
        file: PathBuf,
        /// New mode bits.
        mode: u32,
    },
    /// Delete a file.
    Delete {
        /// File to delete, relative to the package root.
        file: PathBuf,
    },
    /// Move a file from one path to another, atomically when possible.
    /// Creates the destination's parent directory if needed.
    Rename {
        /// Source path, relative to the package root.
        file: PathBuf,
        /// Destination path, relative to the package root.
        to: PathBuf,
    },
    /// Remove a directory if it is empty. A no-op if the directory has
    /// any remaining entries — useful as a follow-up to a `Delete` that
    /// might have been the last file in its parent directory.
    RemoveDirIfEmpty {
        /// Directory to remove, relative to the package root. The
        /// applier reuses the `file` field name for grouping purposes.
        file: PathBuf,
    },
    /// Overwrite (or create) a file with the given content.
    Write {
        /// File to write, relative to the package root.
        file: PathBuf,
        /// Bytes to write.
        content: Vec<u8>,
    },
    /// Replace a byte range in a file.
    ReplaceText {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// Range to replace.
        range: TextRange,
        /// Replacement text.
        replacement: String,
    },
    /// Replace every occurrence of a literal string with another. Operates
    /// on the file's textual content with no awareness of file structure.
    Substitute {
        /// File to edit, relative to the package root.
        file: PathBuf,
        /// String to find (literal, not a regex).
        from: String,
        /// Replacement string.
        to: String,
    },
    /// Normalise the file's line endings to LF (i.e. convert any CRLF
    /// sequences to LF). Carries no payload other than the path: each
    /// applier reads the current file, performs the conversion, and
    /// writes back. Modelling this as its own variant (rather than as a
    /// `Write` carrying the converted bytes) keeps the diagnostic stream
    /// declarative — anyone reading it sees the *intent* and not a byte
    /// blob — and lets an LSP host emit a structural `TextEdit` derived
    /// from the open buffer rather than from a possibly-stale snapshot.
    NormalizeLineEndings {
        /// File to convert, relative to the package root.
        file: PathBuf,
    },
}

/// Identifies a paragraph in a deb822 file.
///
/// The variants are a union of file-format vocabularies. Each variant is
/// labelled with the family of files it applies to; the applier validates
/// that a selector matches the file it's targeting (e.g.
/// [`Binary`](Self::Binary) on `debian/copyright` is an error).
///
/// File-format-agnostic selectors ([`Index`](Self::Index),
/// [`ByKey`](Self::ByKey)) work on any deb822 file, including ones we
/// don't have a typed wrapper for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParagraphSelector {
    /// debian/control: the source paragraph.
    Source,
    /// debian/control: a binary paragraph identified by its `Package:` field.
    Binary {
        /// Package name.
        package: String,
    },
    /// debian/copyright: the header paragraph (carrying `Format:`,
    /// `Upstream-Name:`, etc.).
    CopyrightHeader,
    /// debian/copyright: the paragraph whose `Files:` field matches the
    /// given glob string exactly.
    CopyrightFiles {
        /// Files-glob string, matched literally against the field value.
        glob: String,
    },
    /// debian/copyright: a standalone License paragraph (no `Files:` field)
    /// whose License synopsis equals `name`.
    CopyrightLicense {
        /// License short-name as it appears on the first line of the
        /// `License:` field (e.g. `GPL-2+`).
        name: String,
    },
    /// File-format-agnostic: the Nth paragraph (0-indexed). Use sparingly:
    /// indices shift as paragraphs are inserted or removed.
    Index {
        /// Zero-based paragraph index.
        index: usize,
    },
    /// File-format-agnostic: the first paragraph whose `field` has exactly
    /// the given `value`.
    ByKey {
        /// Field name to match (case-sensitive, as deb822 keys are).
        field: String,
        /// Required value.
        value: String,
    },
}

/// A byte range in a file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextRange {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_serializes_with_kind_tag() {
        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "foo".into(),
            },
            field: "Priority".into(),
            value: "optional".into(),
        });
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(json["kind"], "deb822");
        assert_eq!(json["op"], "set_field");
        assert_eq!(json["field"], "Priority");
        assert_eq!(json["value"], "optional");
        assert_eq!(json["paragraph"]["kind"], "binary");
        assert_eq!(json["paragraph"]["package"], "foo");
    }

    #[test]
    fn action_roundtrips_through_json() {
        let original = Action::Filesystem(FilesystemAction::SetMode {
            file: PathBuf::from("debian/rules"),
            mode: 0o755,
        });
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
