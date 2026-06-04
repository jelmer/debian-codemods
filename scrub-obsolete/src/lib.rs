use crate::action::Action;
use breezyshim::commit::NullCommitReporter;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
use deb822_lossless::Paragraph;
use debian_analyzer::editor::EditorError;
use debian_control::lossless::relations::{Entry, Relation, Relations};
use debian_control::relations::VersionConstraint;
use debian_control::{Binary, Source};
use debian_workspace::action::{
    Action as WsAction, Deb822Action, MaintscriptAction as WsMaintscriptAction, ParagraphSelector,
};
use debian_workspace::appliers::apply_actions;
use debian_workspace::fs_workspace::FsWorkspace;
use debian_workspace::workspace::Workspace;
use debversion::Version;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

pub mod action;
pub mod dummy_transitional;
pub mod package_checker;
pub mod remove_annotations;
use package_checker::{PackageChecker, UddPackageChecker};

/// Represents a field change: (field_name, actions, description)
pub type FieldChange = (String, Vec<Action>, String);

/// Represents changes to a control paragraph: (paragraph_name, field_changes)
pub type ParagraphChanges = (Option<String>, Vec<FieldChange>);

/// A collection of control paragraph changes
pub type ControlChanges = Vec<ParagraphChanges>;

pub const DEFAULT_VALUE_MULTIARCH_HINT: usize = 30;

pub fn note_changelog_policy(policy: bool, msg: &str) {
    lazy_static::lazy_static! {
        static ref CHANGELOG_POLICY_NOTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    }
    if let Ok(mut policy_noted) = CHANGELOG_POLICY_NOTED.lock() {
        if !*policy_noted {
            let extra = if policy {
                "Specify --no-update-changelog to override."
            } else {
                "Specify --update-changelog to override."
            };
            log::info!("{} {}", msg, extra);
        }
        *policy_noted = true;
    }
}

fn depends_obsolete(
    latest_version: &Version,
    kind: VersionConstraint,
    req_version: &Version,
) -> bool {
    match kind {
        VersionConstraint::GreaterThanEqual => latest_version >= req_version,
        VersionConstraint::GreaterThan => latest_version > req_version,
        VersionConstraint::Equal => false,
        _ => false,
    }
}

fn conflict_obsolete(
    latest_version: &Version,
    kind: VersionConstraint,
    req_version: &Version,
) -> bool {
    match kind {
        VersionConstraint::LessThan => latest_version >= req_version,
        VersionConstraint::LessThanEqual | VersionConstraint::Equal => latest_version > req_version,
        _ => false,
    }
}

/// Drop obsolete relations from a relations field.
///
/// # Arguments
/// * `entry` - entry to drop relations from
/// * `checker` - package checker to use to determine if a package is obsolete
/// * `keep_minimum_versions` - whether to keep minimum versions of dependencies
async fn drop_obsolete_depends(
    entry: &mut Entry,
    checker: &dyn PackageChecker,
    keep_minimum_versions: bool,
) -> Result<Vec<Action>, ScrubObsoleteError> {
    let mut actions = vec![];
    let mut to_remove = vec![];
    let mut to_replace = vec![];
    for (i, mut pkgrel) in entry.relations().enumerate() {
        let Some(pkgrel_name) = pkgrel.try_name() else {
            continue;
        };
        if let Some(replacement) = checker.replacement(&pkgrel_name).await? {
            let parsed_replacement: Relations = replacement.parse().unwrap();
            if parsed_replacement.entries().count() > 1 {
                log::warn!("Unable to replace multi-package {:?}", replacement);
            } else {
                let newrel: Entry = replacement.parse().unwrap();
                if debian_analyzer::relations::is_relation_implied(&newrel, entry) {
                    // If the replacement is already included in the entry, we can drop the old
                    // package.
                    to_remove.push(i);
                    actions.push(Action::DropTransition(pkgrel));
                } else {
                    // Otherwise, we can replace the old package with the new one.
                    to_replace.push((i, newrel.relations().next().unwrap()));
                    actions.push(Action::ReplaceTransition(
                        pkgrel,
                        vec![replacement.parse().unwrap()],
                    ))
                }
            }
        } else if pkgrel_name != "debhelper" {
            let compat_version = checker.package_version(&pkgrel_name).await?;
            log::debug!(
                "Relation: {}. Upgrade release {} has {:?} ",
                pkgrel,
                checker.release(),
                compat_version,
            );

            // If the package is essential, we don't need to maintain a dependency on it.
            if checker.is_essential(&pkgrel_name).await?.unwrap_or(false) {
                to_remove.push(i);
                actions.push(Action::DropEssential(pkgrel));
            } else if let Some(pkgrel_version) = pkgrel.version() {
                if compat_version
                    .as_ref()
                    .map(|cv| depends_obsolete(cv, pkgrel_version.0, &pkgrel_version.1))
                    .unwrap_or(false)
                    && !keep_minimum_versions
                {
                    let removed: Relation = pkgrel.to_string().parse().unwrap();
                    pkgrel.set_version(None);
                    actions.push(Action::DropMinimumVersion(removed))
                }
            }
        }
    }

    for (i, newrel) in to_replace {
        entry.replace(i, newrel);
    }

    for i in to_remove.into_iter().rev() {
        entry.remove_relation(i);
    }

    Ok(actions)
}

async fn drop_obsolete_conflicts(
    checker: &dyn PackageChecker,
    entry: &mut Entry,
) -> Result<Vec<Action>, ScrubObsoleteError> {
    let mut to_remove = vec![];
    let mut actions = vec![];
    for (i, pkgrel) in entry.relations().enumerate() {
        let Some(pkgrel_name) = pkgrel.try_name() else {
            continue;
        };
        if let Some((vc, version)) = pkgrel.version() {
            let compat_version = checker.package_version(&pkgrel_name).await?;
            if compat_version
                .map(|cv| conflict_obsolete(&cv, vc, &version))
                .unwrap_or(false)
            {
                actions.push(Action::DropObsoleteConflict(pkgrel));
                to_remove.push(i);
                continue;
            }
        }
    }
    for i in to_remove.into_iter().rev() {
        entry.get_relation(i).unwrap().remove();
    }
    Ok(actions)
}

fn update_depends(
    base: &mut Paragraph,
    field: &str,
    checker: &dyn PackageChecker,
    keep_minimum_versions: bool,
) -> Vec<Action> {
    let mut actions = filter_relations(base, field, |oldrelation: &mut Entry| {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(drop_obsolete_depends(
                oldrelation,
                checker,
                keep_minimum_versions,
            ))
        })
        .unwrap()
    });
    actions.extend(drop_redundant_entries(base, field));
    actions
}

/// Drop alternative entries that have become redundant.
///
/// After version constraints are stripped, an alternative entry such as
/// `libfoo-perl | perl` is redundant if one of its alternatives (here `perl`)
/// is already required unconditionally by another entry in the same field. In
/// that case the whole alternative entry can be dropped. See Debian bug
/// #981529.
fn drop_redundant_entries(base: &mut Paragraph, field: &str) -> Vec<Action> {
    let Some(old_contents) = base.get(field) else {
        return vec![];
    };
    let mut relations: Relations = old_contents.parse().unwrap();

    // Entries that unconditionally require a single package (no alternatives,
    // no version constraint). These can subsume an alternative elsewhere.
    let standalone: Vec<Entry> = relations
        .entries()
        .filter(|entry| {
            let mut rels = entry.relations();
            match (rels.next(), rels.next()) {
                (Some(rel), None) => rel.version().is_none(),
                _ => false,
            }
        })
        .collect();

    let mut to_remove = vec![];
    let mut actions = vec![];
    for (i, entry) in relations.entries().enumerate() {
        // Only alternative groups (more than one option) can be made redundant
        // this way; a single relation is handled by the obsolete-dependency
        // logic instead.
        if entry.relations().count() < 2 {
            continue;
        }
        if standalone
            .iter()
            .any(|s| s != &entry && debian_analyzer::relations::is_relation_implied(s, &entry))
        {
            actions.push(Action::DropRedundant(entry.to_string().parse().unwrap()));
            to_remove.push(i);
        }
    }

    for i in to_remove.into_iter().rev() {
        relations.remove_entry(i);
    }

    if !actions.is_empty() {
        let new_contents = relations.to_string();
        if relations.is_empty() {
            base.remove(field);
        } else {
            base.set(field, &new_contents);
        }
    }
    actions
}

/// Update a relations field.
fn filter_relations(
    base: &mut Paragraph,
    field: &str,
    cb: impl Fn(&mut Entry) -> Vec<Action>,
) -> Vec<Action> {
    let old_contents = base.get(field).unwrap_or_default();

    let relations: Relations = old_contents.parse().unwrap();

    let mut all_actions = vec![];
    for mut entry in relations.entries() {
        let actions = cb(&mut entry);
        all_actions.extend(actions);
    }

    let new_contents = relations.to_string();
    if new_contents != old_contents {
        if relations.is_empty() {
            base.remove(field);
        } else {
            base.set(field, &new_contents);
        }
    }
    all_actions
}

fn update_conflicts(
    base: &mut Paragraph,
    field: &str,
    checker: &dyn PackageChecker,
) -> Vec<Action> {
    filter_relations(base, field, |oldrelation: &mut Entry| -> Vec<Action> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(drop_obsolete_conflicts(checker, oldrelation))
        })
        .unwrap()
    })
}

fn drop_old_source_relations(
    source: &mut Source,
    build_checker: &dyn PackageChecker,
    compat_release: &str,
    keep_minimum_depends_versions: bool,
) -> Vec<(String, Vec<Action>, String)> {
    let mut ret = vec![];
    for field in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"] {
        let actions = update_depends(
            source.as_mut_deb822(),
            field,
            build_checker,
            keep_minimum_depends_versions,
        );
        if !actions.is_empty() {
            ret.push((field.to_string(), actions, compat_release.to_string()))
        }
    }
    for field in [
        "Build-Conflicts",
        "Build-Conflicts-Indep",
        "Build-Conflicts-Arch",
    ] {
        let actions = update_conflicts(source.as_mut_deb822(), field, build_checker);
        if !actions.is_empty() {
            ret.push((field.to_string(), actions, compat_release.to_string()));
        }
    }
    ret
}

fn drop_old_binary_relations(
    runtime_checker: &dyn PackageChecker,
    binary: &mut Binary,
    upgrade_release: &str,
    keep_minimum_depends_versions: bool,
) -> Vec<(String, Vec<Action>, String)> {
    let mut ret = vec![];
    for field in ["Depends", "Suggests", "Recommends", "Pre-Depends"] {
        let actions = update_depends(
            binary.as_mut_deb822(),
            field,
            runtime_checker,
            keep_minimum_depends_versions,
        );
        if !actions.is_empty() {
            ret.push((field.to_string(), actions, upgrade_release.to_string()));
        }
    }

    for field in ["Conflicts", "Replaces", "Breaks"] {
        let actions = update_conflicts(binary.as_mut_deb822(), field, runtime_checker);
        if !actions.is_empty() {
            ret.push((field.to_string(), actions, upgrade_release.to_string()));
        }
    }

    ret
}

/// Detect changes against a parsed control file.
///
/// Walks every source/binary paragraph and runs the scrub-obsolete rules.
/// Returns the per-paragraph changes (used for reporting and translated into
/// `debian_workspace::Action`s for the applier).
fn detect_control_changes(
    control: &debian_control::lossless::Control,
    build_checker: &dyn PackageChecker,
    runtime_checker: &dyn PackageChecker,
    compat_release: &str,
    upgrade_release: &str,
    keep_minimum_depends_versions: bool,
) -> ControlChanges {
    let mut actions = vec![];
    let mut source_actions = vec![];

    if let Some(mut source) = control.source() {
        source_actions.extend(drop_old_source_relations(
            &mut source,
            build_checker,
            compat_release,
            keep_minimum_depends_versions,
        ));
    }

    if !source_actions.is_empty() {
        actions.push((None, source_actions));
    }

    for mut binary in control.binaries() {
        let binary_actions = drop_old_binary_relations(
            runtime_checker,
            &mut binary,
            upgrade_release,
            keep_minimum_depends_versions,
        );
        if !binary_actions.is_empty() {
            actions.push((binary.name(), binary_actions));
        }
    }

    actions
}

/// Translate a scrub-obsolete action into a debian-workspace deb822 action.
///
/// The scrub-obsolete `Action` type is the user-facing reporting format. The
/// applier consumes `debian_workspace::Action`s, so we project the two.
fn action_to_ws(
    action: &Action,
    selector: ParagraphSelector,
    field: &str,
    control_file: &Path,
) -> Option<WsAction> {
    let file = control_file.to_path_buf();
    match action {
        Action::DropEssential(rel)
        | Action::DropTransition(rel)
        | Action::DropObsoleteConflict(rel) => {
            let package = rel.try_name()?;
            Some(WsAction::Deb822(Deb822Action::DropRelation {
                file,
                paragraph: selector,
                field: field.to_string(),
                package,
            }))
        }
        Action::DropMinimumVersion(rel) => {
            let package = rel.try_name()?;
            Some(WsAction::Deb822(
                Deb822Action::SetRelationVersionConstraint {
                    file,
                    paragraph: selector,
                    field: field.to_string(),
                    package,
                    constraint: None,
                },
            ))
        }
        Action::DropRedundant(entry) => Some(WsAction::Deb822(Deb822Action::DropRelationEntry {
            file,
            paragraph: selector,
            field: field.to_string(),
            entry: entry.to_string(),
        })),
        Action::ReplaceTransition(rel, replacements) => {
            let package = rel.try_name()?;
            // scrub-obsolete only emits ReplaceTransition with a single-package
            // replacement (multi-package replacements are warned about and
            // skipped earlier in drop_obsolete_depends).
            let to_entry = replacements
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            Some(WsAction::Deb822(Deb822Action::ReplaceRelation {
                file,
                paragraph: selector,
                field: field.to_string(),
                from_package: package,
                to_entry,
            }))
        }
    }
}

fn control_changes_to_ws_actions(changes: &ControlChanges, control_file: &Path) -> Vec<WsAction> {
    let mut out = vec![];
    for (para, field_changes) in changes {
        let selector = match para {
            None => ParagraphSelector::Source,
            Some(package) => ParagraphSelector::Binary {
                package: package.clone(),
            },
        };
        for (field, actions, _release) in field_changes {
            for action in actions {
                if let Some(a) = action_to_ws(action, selector.clone(), field, control_file) {
                    out.push(a);
                }
            }
        }
    }
    out
}

/// Per-file maintscript entries that should be dropped.
type MaintscriptRemovals = Vec<(PathBuf, Vec<MaintscriptAction>)>;

/// Detect changes to maintscript files in `debian/` (and `<pkg>.maintscript` /
/// `maintscript`). Returns the per-file removed entries plus the workspace
/// actions that will drop them.
#[allow(clippy::result_large_err)]
fn detect_maintscript_changes(
    ws: &dyn Workspace,
    checker: &dyn PackageChecker,
) -> Result<(MaintscriptRemovals, Vec<WsAction>), ScrubObsoleteError> {
    let mut ret = vec![];
    let mut ws_actions = vec![];
    let debian = Path::new("debian");
    let Some(entries) = ws.list_dir(debian)? else {
        return Ok((ret, ws_actions));
    };
    for name in entries {
        if !(name == "maintscript" || name.ends_with(".maintscript")) {
            continue;
        }
        let rel = debian.join(&name);
        let Some(bytes) = ws.read_file(&rel)? else {
            continue;
        };
        let text = String::from_utf8(bytes.into_owned()).map_err(|e| {
            ScrubObsoleteError::Other(format!("{}: invalid UTF-8: {}", rel.display(), e))
        })?;
        let script: debian_analyzer::maintscripts::Maintscript = text.parse().map_err(|e| {
            ScrubObsoleteError::Other(format!("Failed to parse {}: {}", rel.display(), e))
        })?;

        let mut can_drop = |p: &str, v: &Version| -> bool {
            let compat_version = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(checker.package_version(p))
            })
            .unwrap();
            compat_version.map(|cv| &cv > v).unwrap_or(false)
        };

        let removed = drop_obsolete_maintscript_entries(&script, &mut can_drop);
        if removed.is_empty() {
            continue;
        }
        for entry in &removed {
            // Pin the entry's text from the maintscript so the applier can
            // match it line-by-line.
            let entry_text = script
                .entries()
                .get(entry.lineno - 1)
                .map(|e| e.to_string().trim().to_string())
                .unwrap_or_default();
            if entry_text.is_empty() {
                continue;
            }
            ws_actions.push(WsAction::Maintscript(WsMaintscriptAction::DropEntry {
                file: rel.clone(),
                entry: entry_text,
            }));
        }
        ret.push((rel, removed));
    }
    Ok((ret, ws_actions))
}

pub struct MaintscriptAction {
    pub package: String,
    pub version: Version,
    pub lineno: usize,
}

impl serde::Serialize for MaintscriptAction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut ser = serializer.serialize_tuple(3)?;
        ser.serialize_element(&self.lineno)?;
        ser.serialize_element(&self.package)?;
        ser.serialize_element(&self.version)?;
        ser.end()
    }
}

impl<'a> serde::Deserialize<'a> for MaintscriptAction {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        struct MaintscriptActionVisitor;
        impl<'de> serde::de::Visitor<'de> for MaintscriptActionVisitor {
            type Value = MaintscriptAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a tuple of (lineno, package, version)")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let lineno = seq
                    .next_element::<usize>()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &"tuple of 3"))?;
                let package = seq
                    .next_element::<String>()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &"tuple of 3"))?;
                let version = seq
                    .next_element::<Version>()?
                    .ok_or_else(|| serde::de::Error::invalid_length(2, &"tuple of 3"))?;
                Ok(MaintscriptAction {
                    package,
                    version,
                    lineno,
                })
            }
        }
        deserializer.deserialize_tuple(3, MaintscriptActionVisitor)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ScrubObsoleteResult {
    specific_files: Vec<PathBuf>,
    control_actions: ControlChanges,
    maintscript_removed: Vec<(PathBuf, Vec<MaintscriptAction>, String)>,
}

impl ScrubObsoleteResult {
    pub fn any_changes(&self) -> bool {
        !self.control_actions.is_empty() || !self.maintscript_removed.is_empty()
    }

    pub fn value(&self) -> i32 {
        let mut value = DEFAULT_VALUE_MULTIARCH_HINT;
        for (_para, changes) in &self.control_actions {
            for (_field, actions, _) in changes {
                value += actions.len() * 2;
            }
        }
        for (_, removed, _) in &self.maintscript_removed {
            value += removed.len();
        }
        value as i32
    }

    pub fn itemized(&self) -> HashMap<String, Vec<String>> {
        let mut summary = HashMap::new();
        for (para, changes) in &self.control_actions {
            for (field, actions, release) in changes {
                for action in actions {
                    if let Some(para) = para {
                        summary
                            .entry(release.to_string())
                            .or_insert_with(Vec::new)
                            .push(format!("{}: {} in {}.", para, action, field));
                    } else {
                        summary
                            .entry(release.to_string())
                            .or_insert_with(Vec::new)
                            .push(format!("{}: {}.", field, action));
                    }
                }
            }
        }
        if !self.maintscript_removed.is_empty() {
            let total_entries: usize = self
                .maintscript_removed
                .iter()
                .map(|(_, entries, _)| entries.len())
                .sum();
            summary
                .entry(self.maintscript_removed[0].2.clone())
                .or_insert_with(Vec::new)
                .push(format!(
                    "Remove {} maintscript entries from {} files.",
                    total_entries,
                    self.maintscript_removed.len()
                ));
        }
        summary
    }
}

/// Detect every scrub-obsolete change against a workspace.
///
/// Returns the result with no changes applied. Callers that want to apply the
/// changes should pass `result.workspace_actions()` to
/// [`apply_actions`](debian_workspace::appliers::apply_actions).
pub async fn detect_scrub_obsolete(
    ws: &dyn Workspace,
    compat_release: &str,
    upgrade_release: &str,
    keep_minimum_depends_versions: bool,
) -> Result<DetectedChanges, ScrubObsoleteError> {
    let source_package_checker = UddPackageChecker::new(compat_release, true).await?;
    let binary_package_checker = UddPackageChecker::new(upgrade_release, false).await?;

    let control_file_rel = Path::new("debian/control");
    let (control_actions, control_ws_actions) = if ws.parsed_debcargo()?.is_some() {
        // debcargo-managed packages: skip control entirely.
        (vec![], vec![])
    } else {
        let control = match ws.parsed_control() {
            Ok(c) => c,
            Err(debian_workspace::Error::NotFound) => {
                return Err(ScrubObsoleteError::NotDebianPackage(PathBuf::from(
                    "debian",
                )));
            }
            Err(e) => return Err(ScrubObsoleteError::Workspace(e)),
        };
        let changes = detect_control_changes(
            &control,
            &source_package_checker,
            &binary_package_checker,
            compat_release,
            upgrade_release,
            keep_minimum_depends_versions,
        );
        let ws_actions = control_changes_to_ws_actions(&changes, control_file_rel);
        (changes, ws_actions)
    };

    let (maintscript_removed, maintscript_ws_actions) =
        detect_maintscript_changes(ws, &binary_package_checker)?;

    let mut workspace_actions = control_ws_actions;
    workspace_actions.extend(maintscript_ws_actions);

    let maintscript_removed = maintscript_removed
        .into_iter()
        .map(|(path, removed)| (path, removed, upgrade_release.to_string()))
        .collect();

    Ok(DetectedChanges {
        control_actions,
        maintscript_removed,
        workspace_actions,
    })
}

/// Output of [`detect_scrub_obsolete`]: the per-file edits plus the typed
/// `debian_workspace::Action`s that, when applied, perform them.
pub struct DetectedChanges {
    /// Per-paragraph, per-field scrub-obsolete actions (for reporting).
    pub control_actions: ControlChanges,
    /// Per-file maintscript entries to drop, with the release they're keyed
    /// against (third tuple element).
    pub maintscript_removed: Vec<(PathBuf, Vec<MaintscriptAction>, String)>,
    /// Typed workspace actions that produce these changes.
    pub workspace_actions: Vec<WsAction>,
}

impl DetectedChanges {
    pub fn any_changes(&self) -> bool {
        !self.control_actions.is_empty() || !self.maintscript_removed.is_empty()
    }
}

#[derive(Debug)]
pub enum ScrubObsoleteError {
    NotDebianPackage(PathBuf),
    EditorError(EditorError),
    BrzError(BrzError),
    SqlxError(sqlx::Error),
    IoError(std::io::Error),
    /// A debian-workspace error surfaced from the workspace abstraction or
    /// the applier.
    Workspace(debian_workspace::Error),
    /// Catch-all for errors that don't map onto one of the typed variants.
    Other(String),
}

impl std::fmt::Display for ScrubObsoleteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ScrubObsoleteError::NotDebianPackage(path) => {
                write!(f, "Not a Debian package: {:?}", path)
            }
            ScrubObsoleteError::EditorError(e) => write!(f, "Editor error: {}", e),
            ScrubObsoleteError::BrzError(e) => write!(f, "Breezy error: {}", e),
            ScrubObsoleteError::SqlxError(e) => write!(f, "SQLx error: {}", e),
            ScrubObsoleteError::IoError(e) => write!(f, "I/O error: {}", e),
            ScrubObsoleteError::Workspace(e) => write!(f, "Workspace error: {}", e),
            ScrubObsoleteError::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ScrubObsoleteError {}

impl From<EditorError> for ScrubObsoleteError {
    fn from(e: EditorError) -> Self {
        ScrubObsoleteError::EditorError(e)
    }
}

impl From<BrzError> for ScrubObsoleteError {
    fn from(e: BrzError) -> Self {
        ScrubObsoleteError::BrzError(e)
    }
}

impl From<sqlx::Error> for ScrubObsoleteError {
    fn from(e: sqlx::Error) -> Self {
        ScrubObsoleteError::SqlxError(e)
    }
}

impl From<std::io::Error> for ScrubObsoleteError {
    fn from(e: std::io::Error) -> Self {
        ScrubObsoleteError::IoError(e)
    }
}

impl From<debian_workspace::Error> for ScrubObsoleteError {
    fn from(e: debian_workspace::Error) -> Self {
        ScrubObsoleteError::Workspace(e)
    }
}

/// Scrub obsolete entries.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::result_large_err)]
pub fn scrub_obsolete(
    wt: &GenericWorkingTree,
    subpath: &Path,
    compat_release: &str,
    upgrade_release: &str,
    update_changelog: Option<bool>,
    #[allow(unused_variables)] allow_reformatting: bool,
    keep_minimum_depends_versions: bool,
    #[allow(unused_variables)] transitions: Option<HashMap<String, String>>,
) -> Result<ScrubObsoleteResult, ScrubObsoleteError> {
    let debian_path = subpath.join("debian");
    let base_path = wt.abspath(subpath)?;

    // scrub-obsolete doesn't surface package/version metadata to its
    // detectors, so leave them unset rather than fabricating sentinels.
    let ws = FsWorkspace::new(&base_path, None, None);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let detected = rt.block_on(detect_scrub_obsolete(
        &ws,
        compat_release,
        upgrade_release,
        keep_minimum_depends_versions,
    ))?;

    let mut result = ScrubObsoleteResult {
        specific_files: vec![],
        control_actions: detected.control_actions,
        maintscript_removed: detected.maintscript_removed,
    };

    if !result.any_changes() {
        return Ok(result);
    }

    let changed_files = apply_actions(ws.base_path(), &detected.workspace_actions)?;
    // The applier returns paths relative to base_path; promote them to
    // tree-relative paths via the breezy working tree.
    let safe_files: Vec<&Path> = changed_files.iter().map(|p| p.as_path()).collect();
    let mut specific_files: Vec<PathBuf> = wt
        .safe_relpath_files(safe_files.as_slice(), true, false)?
        .into_iter()
        .collect();

    let summary = result.itemized();

    let changelog_path = debian_path.join("changelog");

    let update_changelog = if let Some(update_changelog) = update_changelog {
        update_changelog
    } else if let Some(dch_guess) =
        debian_analyzer::detect_gbp_dch::guess_update_changelog(wt, &debian_path, None)
    {
        note_changelog_policy(dch_guess.update_changelog, &dch_guess.explanation);
        dch_guess.update_changelog
    } else {
        // If we can't guess, default to updating the changelog.
        true
    };

    if update_changelog {
        let mut lines = vec![];
        for (release, entries) in summary.iter() {
            let rev_aliases = debian_analyzer::release_info::release_aliases(release, None);
            let mut line = format!("Remove constraints unnecessary since {}", release);
            for alias in rev_aliases {
                line += &format!(" ({})", alias);
            }
            line += ":";
            lines.push(line);
            lines.extend(entries.iter().map(|x| format!("* {}", x)));
        }
        debian_analyzer::add_changelog_entry(
            wt,
            &changelog_path,
            lines
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        specific_files.push(changelog_path);
    }

    result.specific_files = specific_files.clone();

    let mut lines = vec![];
    for (release, _entries) in summary.iter() {
        let rev_aliases = debian_analyzer::release_info::release_aliases(release, None);
        let mut line = format!("Remove constraints unnecessary since {}", release);
        for alias in rev_aliases {
            line += &format!(" ({})", alias);
        }
        line += ":";

        lines.push(line);
    }
    lines.extend(["".to_string(), "Changes-By: deb-scrub-obsolete".to_string()]);

    let committer = debian_analyzer::get_committer(wt);

    match wt
        .build_commit()
        .specific_files(
            specific_files
                .iter()
                .map(|x| x.as_path())
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .message(&lines.join("\n"))
        .allow_pointless(false)
        .reporter(&NullCommitReporter::new())
        .committer(&committer)
        .commit()
    {
        Ok(_) | Err(BrzError::PointlessCommit) => {}
        Err(e) => {
            return Err(e.into());
        }
    }

    Ok(result)
}

/// Identify obsolete entries in a maintscript file.
///
/// # Arguments
/// * `script` - parsed maintscript
/// * `should_remove` - callable to check whether a package/version tuple is obsolete
///
/// # Returns
/// list of `MaintscriptAction` records describing the entries that should be
/// dropped (their `lineno` is the 1-based entry index)
fn drop_obsolete_maintscript_entries(
    script: &debian_analyzer::maintscripts::Maintscript,
    should_remove: &mut dyn FnMut(&str, &Version) -> bool,
) -> Vec<MaintscriptAction> {
    let mut ret = vec![];
    for (i, entry) in script.entries().iter().enumerate() {
        if let (Some(package), Some(version)) = (entry.package(), entry.prior_version()) {
            if should_remove(package, version) {
                ret.push(MaintscriptAction {
                    package: package.clone(),
                    version: version.clone(),
                    lineno: i + 1,
                });
            }
        }
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use deb822_lossless::Paragraph;
    use std::collections::{HashMap, HashSet};

    #[cfg(test)]
    mod test_filter_relations {
        use super::*;
        #[test]
        fn test_missing() {
            let mut control = Paragraph::new();
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |_| vec![])
            );
        }

        #[test]
        fn test_keep() {
            let mut control = Paragraph::new();
            control.set("Depends", "foo");
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |_oldrel| vec![])
            );
        }

        #[test]
        fn test_drop_last() {
            let mut control = Paragraph::new();
            control.set("Depends", "foo");
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |oldrel| {
                    oldrel.remove();
                    vec![]
                })
            );
            assert_eq!(control.get("Depends"), None);
        }

        #[test]
        fn test_drop_first() {
            let mut control = Paragraph::new();
            control.set("Depends", "foo, bar");
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |oldrel| {
                    if oldrel.relations().next().unwrap().try_name().as_deref() == Some("foo") {
                        oldrel.remove();
                        vec![]
                    } else {
                        vec![]
                    }
                })
            );
            assert_eq!(control.get("Depends").as_deref(), Some("bar"));
        }

        #[test]
        fn test_keep_last_comma() {
            let mut control = Paragraph::new();
            control.set("Depends", "foo, bar, ");
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |oldrel| {
                    if oldrel.relations().next().unwrap().try_name().as_deref() == Some("foo") {
                        oldrel.remove();
                        vec![]
                    } else {
                        vec![]
                    }
                })
            );
            assert_eq!(control.get("Depends").as_deref(), Some("bar, "));
        }

        #[test]
        fn test_drop_just_comma() {
            let mut control = Paragraph::new();
            control.set("Depends", "foo, ");
            assert_eq!(
                Vec::<Action>::new(),
                filter_relations(&mut control, "Depends", |oldrel| {
                    if oldrel.relations().next().unwrap().try_name().as_deref() == Some("foo") {
                        oldrel.remove();
                        vec![]
                    } else {
                        vec![]
                    }
                })
            );
            assert_eq!(control.get("Depends"), None);
        }
    }

    struct DummyChecker<'a> {
        versions: HashMap<&'a str, Version>,
        essential: HashSet<&'a str>,
        transitions: HashMap<&'a str, &'a str>,
    }

    #[async_trait]
    impl crate::package_checker::PackageChecker for DummyChecker<'_> {
        fn release(&self) -> &str {
            "release"
        }

        async fn package_version(&self, package: &str) -> Result<Option<Version>, sqlx::Error> {
            Ok(self.versions.get(package).cloned())
        }

        async fn replacement(&self, package: &str) -> Result<Option<String>, sqlx::Error> {
            Ok(self.transitions.get(package).map(|x| x.to_string()))
        }

        async fn package_provides(
            &self,
            _package: &str,
        ) -> Result<Vec<(String, Option<Version>)>, sqlx::Error> {
            unimplemented!()
        }

        async fn is_essential(&self, package: &str) -> Result<Option<bool>, sqlx::Error> {
            Ok(Some(self.essential.contains(package)))
        }
    }

    mod test_drop_obsolete_depends {
        use super::*;

        #[tokio::test]
        async fn test_empty() {
            let mut entry = Entry::new();
            assert_eq!(
                Vec::<Action>::new(),
                drop_obsolete_depends(
                    &mut entry,
                    &DummyChecker {
                        versions: HashMap::new(),
                        essential: HashSet::new(),
                        transitions: HashMap::new()
                    },
                    false
                )
                .await
                .unwrap()
            );
        }

        #[tokio::test]
        async fn test_single() {
            let checker = DummyChecker {
                versions: maplit::hashmap! {"simple" => "1.1".parse().unwrap()},
                essential: HashSet::new(),
                transitions: HashMap::new(),
            };
            let mut entry: Entry = "simple (>= 1.0)".parse().unwrap();
            let actions = drop_obsolete_depends(&mut entry, &checker, false)
                .await
                .unwrap();
            assert_eq!(
                vec![Action::DropMinimumVersion(
                    "simple (>= 1.0)".parse().unwrap()
                )],
                actions
            );
            assert_eq!(entry.relations().count(), 1);
        }

        #[tokio::test]
        async fn test_essential() {
            let checker = DummyChecker {
                versions: maplit::hashmap!["simple" => "1.1".parse().unwrap()],
                essential: maplit::hashset!["simple"],
                transitions: HashMap::new(),
            };
            let mut entry: Entry = "simple (>= 1.0)".parse().unwrap();
            let actions = drop_obsolete_depends(&mut entry, &checker, false)
                .await
                .unwrap();
            assert_eq!(
                vec![Action::DropEssential("simple (>= 1.0)".parse().unwrap())],
                actions
            );
            assert_eq!(entry.to_string(), "");
        }

        #[tokio::test]
        async fn test_debhelper() {
            let checker = DummyChecker {
                versions: maplit::hashmap!["debhelper" => "1.4".parse().unwrap()],
                essential: HashSet::new(),
                transitions: HashMap::new(),
            };
            let mut entry: Entry = "debhelper (>= 1.1)".parse().unwrap();
            assert_eq!(
                Vec::<Action>::new(),
                drop_obsolete_depends(&mut entry, &checker, false)
                    .await
                    .unwrap()
            );
            assert_eq!(entry.relations().count(), 1);
        }

        #[tokio::test]
        async fn test_other_essential() {
            let checker = DummyChecker {
                versions: maplit::hashmap!["simple" => "1.1".parse().unwrap()],
                essential: maplit::hashset!["simple"],
                transitions: HashMap::new(),
            };
            let mut entry: Entry = "simple (>= 1.0) | other".parse().unwrap();
            let actions = drop_obsolete_depends(&mut entry, &checker, false)
                .await
                .unwrap();

            assert_eq!(
                vec![Action::DropEssential("simple (>= 1.0)".parse().unwrap())],
                actions
            );
            assert_eq!(entry.to_string(), "other");
        }

        #[tokio::test]
        async fn test_transition() {
            let checker = DummyChecker {
                versions: maplit::hashmap! {"simple" => "1.1".parse().unwrap()},
                essential: maplit::hashset!["simple"],
                transitions: maplit::hashmap! {"oldpackage" => "replacement"},
            };
            let mut entry: Entry = "oldpackage (>= 1.0) | other".parse().unwrap();
            assert_eq!(
                vec![Action::ReplaceTransition(
                    "oldpackage (>= 1.0)".parse().unwrap(),
                    vec!["replacement".parse().unwrap()]
                )],
                drop_obsolete_depends(&mut entry, &checker, false)
                    .await
                    .unwrap()
            );
            assert_eq!(entry.to_string(), "replacement | other");
        }

        #[tokio::test]
        async fn test_transition_matches() {
            let checker = DummyChecker {
                versions: maplit::hashmap! {"simple" => "1.1".parse().unwrap()},
                essential: maplit::hashset!["simple"],
                transitions: maplit::hashmap! {"oldpackage" => "replacement"},
            };
            let mut entry: Entry = "oldpackage (>= 1.0) | replacement".parse().unwrap();
            assert_eq!(
                vec![Action::DropTransition(
                    "oldpackage (>= 1.0)".parse().unwrap()
                )],
                drop_obsolete_depends(&mut entry, &checker, false)
                    .await
                    .unwrap()
            );
            assert_eq!(entry.to_string(), "replacement");
        }

        #[tokio::test]
        async fn test_transition_dupes() {
            let checker = DummyChecker {
                versions: maplit::hashmap! {"simple" => "1.1".parse().unwrap()},
                essential: maplit::hashset!["simple"],
                transitions: maplit::hashmap! {"oldpackage" => "replacement"},
            };
            let mut entry: Entry = "oldpackage (>= 1.0) | oldpackage (= 3.0) | other"
                .parse()
                .unwrap();
            assert_eq!(
                vec![
                    Action::ReplaceTransition(
                        "oldpackage (>= 1.0)".parse().unwrap(),
                        vec!["replacement".parse().unwrap()]
                    ),
                    Action::ReplaceTransition(
                        "oldpackage (= 3.0)".parse().unwrap(),
                        vec!["replacement".parse().unwrap()]
                    )
                ],
                drop_obsolete_depends(&mut entry, &checker, false)
                    .await
                    .unwrap()
            );
            assert_eq!(entry.to_string(), "replacement | replacement | other");
        }
    }

    mod test_drop_redundant_entries {
        use super::*;

        #[test]
        fn test_empty() {
            let mut control = Paragraph::new();
            assert_eq!(
                Vec::<Action>::new(),
                drop_redundant_entries(&mut control, "Depends")
            );
        }

        #[test]
        fn test_no_redundancy() {
            let mut control = Paragraph::new();
            control.set("Depends", "perl, libfoo-perl | libbar-perl");
            assert_eq!(
                Vec::<Action>::new(),
                drop_redundant_entries(&mut control, "Depends")
            );
            assert_eq!(
                control.get("Depends").as_deref(),
                Some("perl, libfoo-perl | libbar-perl")
            );
        }

        #[test]
        fn test_redundant_alternative() {
            let mut control = Paragraph::new();
            control.set("Depends", "perl, libfoo-perl | perl");
            assert_eq!(
                vec![Action::DropRedundant("libfoo-perl | perl".parse().unwrap())],
                drop_redundant_entries(&mut control, "Depends")
            );
            assert_eq!(control.get("Depends").as_deref(), Some("perl"));
        }

        #[test]
        fn test_versioned_standalone_not_subsuming() {
            // A versioned standalone dependency does not unconditionally cover
            // the alternative, so the entry is kept.
            let mut control = Paragraph::new();
            control.set("Depends", "perl (>= 5.10), libfoo-perl | perl");
            assert_eq!(
                Vec::<Action>::new(),
                drop_redundant_entries(&mut control, "Depends")
            );
        }

        #[test]
        fn test_single_relation_ignored() {
            // A single relation is left for the obsolete-dependency logic; the
            // redundancy pass only touches alternative groups.
            let mut control = Paragraph::new();
            control.set("Depends", "perl");
            assert_eq!(
                Vec::<Action>::new(),
                drop_redundant_entries(&mut control, "Depends")
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_strip_then_drop_redundant() {
            // The full case from Debian bug #981529: stripping the version
            // constraint turns "perl (>> 5.6.0)" into "perl", making the whole
            // alternative entry redundant.
            let checker = DummyChecker {
                versions: maplit::hashmap! {"perl" => "5.36".parse().unwrap()},
                essential: HashSet::new(),
                transitions: HashMap::new(),
            };
            let mut control = Paragraph::new();
            control.set("Depends", "perl, libfoo-perl | perl (>> 5.6.0)");
            let actions = update_depends(&mut control, "Depends", &checker, false);
            assert_eq!(
                vec![
                    Action::DropMinimumVersion("perl (>> 5.6.0)".parse().unwrap()),
                    Action::DropRedundant("libfoo-perl | perl".parse().unwrap()),
                ],
                actions
            );
            assert_eq!(control.get("Depends").as_deref(), Some("perl"));
        }
    }
}
