//! Apply [`Action`]s to a working tree.
//!
//! See `doc/detector-action-split.md` for the design rationale.
//!
//! Actions for the same file are batched into a single editor session so a
//! detector that emits e.g. one `RemoveField` per binary plus a `SetField`
//! on the source produces a single rewrite of `debian/control`.

use crate::diagnostic::{
    Action, ChangelogAction, Deb822Action, Dep3Action, DesktopIniAction, FilesystemAction,
    LintianOverridesAction, MakefileAction, OverrideLineSelector, ParagraphSelector, SystemdAction,
    WatchAction, YamlAction, YamlPathComponent,
};
use crate::FixerError;
use debian_analyzer::control::TemplatedControlEditor;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Apply a batch of actions to `base_path`.
///
/// Actions are grouped by target file so each file is opened/edited/committed
/// once. The grouping is stable: actions on the same file are applied in the
/// order they appear in `actions`.
///
/// Returns the list of files that were modified. An empty result means no
/// action produced an observable change (e.g. all `RemoveField`s targeted
/// fields that were already absent).
pub fn apply_actions(base_path: &Path, actions: &[Action]) -> Result<Vec<PathBuf>, FixerError> {
    // Group while preserving order.
    let mut groups: BTreeMap<PathBuf, Vec<&Action>> = BTreeMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    for action in actions {
        let file = action_file(action).to_path_buf();
        if !groups.contains_key(&file) {
            order.push(file.clone());
        }
        groups.entry(file).or_default().push(action);
    }

    let mut changed = Vec::new();
    for file in order {
        let group = groups.remove(&file).unwrap();
        let modified = apply_group(base_path, &file, &group)?;
        if modified {
            changed.push(file);
        }
    }
    Ok(changed)
}

/// Convenience: apply a single action.
pub fn apply_action(base_path: &Path, action: &Action) -> Result<bool, FixerError> {
    let changed = apply_actions(base_path, std::slice::from_ref(action))?;
    Ok(!changed.is_empty())
}

fn action_file(action: &Action) -> &Path {
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
            | Deb822Action::ReplaceRelation { file, .. }
            | Deb822Action::EnsureSubstvar { file, .. }
            | Deb822Action::DropSubstvar { file, .. }
            | Deb822Action::EnsureRelation { file, .. }
            | Deb822Action::MoveRelation { file, .. }
            | Deb822Action::ReorderParagraphs { file, .. } => file,
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
            LintianOverridesAction::DropLine { file, .. }
            | LintianOverridesAction::RenameTag { file, .. }
            | LintianOverridesAction::SetLineInfo { file, .. } => file,
        },
        Action::Filesystem(a) => match a {
            FilesystemAction::SetMode { file, .. }
            | FilesystemAction::Delete { file }
            | FilesystemAction::Rename { file, .. }
            | FilesystemAction::RemoveDirIfEmpty { file }
            | FilesystemAction::Write { file, .. }
            | FilesystemAction::ReplaceText { file, .. }
            | FilesystemAction::Substitute { file, .. } => file,
        },
    }
}

fn apply_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    // Decide which applier to use based on the kinds present. We don't allow
    // mixing kinds for the same file (e.g. a Deb822 SetField alongside a
    // Filesystem Delete on debian/control) — that is almost certainly a bug
    // in the detector.
    let mut kinds = std::collections::HashSet::new();
    for action in group {
        kinds.insert(std::mem::discriminant(*action));
    }
    if kinds.len() != 1 {
        return Err(FixerError::Other(format!(
            "Mixed action kinds for {} are not supported",
            rel.display()
        )));
    }
    match group[0] {
        Action::Deb822(_) => apply_deb822_group(base, rel, group),
        Action::Systemd(_) => apply_systemd_group(base, rel, group),
        Action::DesktopIni(_) => apply_desktop_ini_group(base, rel, group),
        Action::Yaml(_) => apply_yaml_group(base, rel, group),
        Action::Changelog(_) => apply_changelog_group(base, rel, group),
        Action::Watch(_) => apply_watch_group(base, rel, group),
        Action::Makefile(_) => apply_makefile_group(base, rel, group),
        Action::Dep3(_) => apply_dep3_group(base, rel, group),
        Action::LintianOverrides(_) => apply_lintian_overrides_group(base, rel, group),
        Action::Filesystem(_) => apply_filesystem_group(base, rel, group),
    }
}

fn apply_deb822_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    // Selectors are tagged with the file family they belong to. We dispatch
    // on the first selector in the group: Source/Binary go through the
    // typed control editor (which applies canonical field ordering on
    // insert); CopyrightHeader/CopyrightFiles go through the typed
    // copyright editor (DEP-5 field ordering); Index/ByKey use the
    // generic deb822 editor. AppendParagraph carries no selector and
    // always uses the generic path.
    let first = first_selector(group);
    if matches!(
        first,
        Some(ParagraphSelector::Source | ParagraphSelector::Binary { .. })
    ) {
        return apply_control_deb822_group(base, rel, group);
    }
    if matches!(
        first,
        Some(ParagraphSelector::CopyrightHeader | ParagraphSelector::CopyrightFiles { .. })
    ) {
        return apply_copyright_deb822_group(base, rel, group);
    }
    apply_generic_deb822_group(base, rel, group)
}

fn first_selector<'a>(group: &'a [&'a Action]) -> Option<&'a ParagraphSelector> {
    for action in group {
        let Action::Deb822(deb) = action else {
            continue;
        };
        return match deb {
            Deb822Action::SetField { paragraph, .. }
            | Deb822Action::SetFieldWithIndent { paragraph, .. }
            | Deb822Action::RemoveField { paragraph, .. }
            | Deb822Action::RenameField { paragraph, .. }
            | Deb822Action::RemoveParagraph { paragraph, .. }
            | Deb822Action::NormalizeFieldSpacing { paragraph, .. }
            | Deb822Action::DropRelation { paragraph, .. }
            | Deb822Action::ReplaceRelation { paragraph, .. }
            | Deb822Action::EnsureSubstvar { paragraph, .. }
            | Deb822Action::DropSubstvar { paragraph, .. }
            | Deb822Action::EnsureRelation { paragraph, .. }
            | Deb822Action::MoveRelation { paragraph, .. } => Some(paragraph),
            Deb822Action::AppendParagraph { .. } | Deb822Action::ReorderParagraphs { .. } => None,
        };
    }
    None
}

fn apply_control_deb822_group(
    base: &Path,
    rel: &Path,
    group: &[&Action],
) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "deb822 action targets missing file {}",
            rel.display()
        )));
    }
    let mut editor = TemplatedControlEditor::open(&abs)?;
    let mut any_change = false;

    for action in group {
        let Action::Deb822(deb) = action else {
            unreachable!("apply_control_deb822_group called with non-deb822 action");
        };
        match deb {
            Deb822Action::SetField {
                paragraph,
                field,
                value,
                ..
            } => {
                if set_deb822_field(&editor, paragraph, field, value, None)? {
                    any_change = true;
                }
            }
            Deb822Action::SetFieldWithIndent {
                paragraph,
                field,
                value,
                indent,
                ..
            } => {
                if set_deb822_field(&editor, paragraph, field, value, Some(indent))? {
                    any_change = true;
                }
            }
            Deb822Action::RemoveField {
                paragraph, field, ..
            } => {
                if remove_deb822_field(&editor, paragraph, field)? {
                    any_change = true;
                }
            }
            Deb822Action::RenameField {
                paragraph,
                from,
                to,
                ..
            } => {
                if rename_deb822_field(&editor, paragraph, from, to)? {
                    any_change = true;
                }
            }
            Deb822Action::RemoveParagraph { paragraph, .. } => {
                if let ParagraphSelector::Binary { package } = paragraph {
                    if editor.remove_binary(package) {
                        any_change = true;
                    }
                } else {
                    return Err(FixerError::Other(format!(
                        "deb822 RemoveParagraph not supported on debian/control for selector {:?}",
                        paragraph
                    )));
                }
            }
            Deb822Action::AppendParagraph { .. } => {
                return Err(FixerError::Other(
                    "deb822 AppendParagraph not supported on debian/control via the typed editor"
                        .into(),
                ));
            }
            Deb822Action::NormalizeFieldSpacing {
                paragraph, field, ..
            } => {
                if normalize_deb822_field_spacing(&editor, paragraph, field)? {
                    any_change = true;
                }
            }
            Deb822Action::DropRelation {
                paragraph,
                field,
                package,
                ..
            } => {
                if drop_deb822_relation(&editor, paragraph, field, package)? {
                    any_change = true;
                }
            }
            Deb822Action::ReplaceRelation {
                paragraph,
                field,
                from_package,
                to_entry,
                ..
            } => {
                if replace_deb822_relation(&editor, paragraph, field, from_package, to_entry)? {
                    any_change = true;
                }
            }
            Deb822Action::EnsureSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => {
                if ensure_deb822_substvar(&editor, paragraph, field, substvar)? {
                    any_change = true;
                }
            }
            Deb822Action::DropSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => {
                if drop_deb822_substvar(&editor, paragraph, field, substvar)? {
                    any_change = true;
                }
            }
            Deb822Action::EnsureRelation {
                paragraph,
                field,
                entry,
                ..
            } => {
                if ensure_deb822_relation(&editor, paragraph, field, entry)? {
                    any_change = true;
                }
            }
            Deb822Action::MoveRelation {
                paragraph,
                from_field,
                to_field,
                package,
                ..
            } => {
                if move_deb822_relation(&editor, paragraph, from_field, to_field, package)? {
                    any_change = true;
                }
            }
            Deb822Action::ReorderParagraphs { .. } => {
                return Err(FixerError::Other(
                    "deb822 ReorderParagraphs is not supported via the typed control editor; use a generic-path action group".into(),
                ));
            }
        }
    }

    if any_change {
        editor.commit()?;
    }
    Ok(any_change)
}

/// Apply deb822 actions targeting `debian/copyright` paragraphs through
/// the typed `Copyright` editor, so SetField/RemoveField on the header
/// or Files paragraphs honour DEP-5 field ordering.
fn apply_copyright_deb822_group(
    base: &Path,
    rel: &Path,
    group: &[&Action],
) -> Result<bool, FixerError> {
    use std::str::FromStr;

    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "deb822 action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    // The typed Copyright parser rejects legacy headers (e.g. files
    // using `Format-Specification` instead of `Format`). Those are
    // exactly the kind of file the fixers need to migrate, so fall back
    // to the generic deb822 path when the typed parser refuses.
    let Ok(copyright) = debian_copyright::lossless::Copyright::from_str(&content) else {
        return apply_generic_deb822_group(base, rel, group);
    };

    // If any action targets something other than the typed-copyright
    // selectors, fall back to the generic deb822 path (which handles
    // ByKey, Index, etc.). The typed path is only worth the round-trip
    // when every action speaks the typed selectors.
    if group.iter().any(|a| {
        let Action::Deb822(deb) = a else {
            return true;
        };
        let p = match deb {
            Deb822Action::SetField { paragraph, .. }
            | Deb822Action::SetFieldWithIndent { paragraph, .. }
            | Deb822Action::RemoveField { paragraph, .. }
            | Deb822Action::RenameField { paragraph, .. }
            | Deb822Action::RemoveParagraph { paragraph, .. }
            | Deb822Action::NormalizeFieldSpacing { paragraph, .. }
            | Deb822Action::DropRelation { paragraph, .. }
            | Deb822Action::ReplaceRelation { paragraph, .. }
            | Deb822Action::EnsureSubstvar { paragraph, .. }
            | Deb822Action::DropSubstvar { paragraph, .. }
            | Deb822Action::EnsureRelation { paragraph, .. }
            | Deb822Action::MoveRelation { paragraph, .. } => paragraph,
            Deb822Action::AppendParagraph { .. } | Deb822Action::ReorderParagraphs { .. } => {
                return false;
            }
        };
        !matches!(
            p,
            ParagraphSelector::CopyrightHeader | ParagraphSelector::CopyrightFiles { .. }
        )
    }) {
        return apply_generic_deb822_group(base, rel, group);
    }

    let mut any_change = false;
    for action in group {
        let Action::Deb822(deb) = action else {
            unreachable!("apply_copyright_deb822_group called with non-deb822 action");
        };
        match deb {
            Deb822Action::SetField {
                paragraph,
                field,
                value,
                ..
            } => match paragraph {
                ParagraphSelector::CopyrightHeader => {
                    let Some(mut header) = copyright.header() else {
                        return Err(FixerError::Other(format!(
                            "deb822 SetField on {}: no header paragraph",
                            rel.display()
                        )));
                    };
                    if header.as_deb822().get(field).as_deref() == Some(value.as_str()) {
                        continue;
                    }
                    header.set_field(field, value);
                    any_change = true;
                }
                ParagraphSelector::CopyrightFiles { glob } => {
                    let Some(mut files_para) = copyright
                        .iter_files()
                        .find(|p| p.as_deb822().get("Files").as_deref() == Some(glob.as_str()))
                    else {
                        return Err(FixerError::Other(format!(
                            "deb822 SetField on {}: no Files paragraph for glob {:?}",
                            rel.display(),
                            glob
                        )));
                    };
                    if files_para.as_deb822().get(field).as_deref() == Some(value.as_str()) {
                        continue;
                    }
                    files_para.set_field(field, value);
                    any_change = true;
                }
                other => {
                    return Err(FixerError::Other(format!(
                        "Copyright SetField does not support paragraph selector {:?}",
                        other
                    )));
                }
            },
            Deb822Action::RemoveField {
                paragraph, field, ..
            } => match paragraph {
                ParagraphSelector::CopyrightHeader => {
                    let Some(mut header) = copyright.header() else {
                        continue;
                    };
                    if header.as_deb822().get(field).is_some() {
                        header.remove_field(field);
                        any_change = true;
                    }
                }
                ParagraphSelector::CopyrightFiles { glob } => {
                    if let Some(mut files_para) = copyright
                        .iter_files()
                        .find(|p| p.as_deb822().get("Files").as_deref() == Some(glob.as_str()))
                    {
                        if files_para.as_deb822().get(field).is_some() {
                            files_para.remove_field(field);
                            any_change = true;
                        }
                    }
                }
                other => {
                    return Err(FixerError::Other(format!(
                        "Copyright RemoveField does not support paragraph selector {:?}",
                        other
                    )));
                }
            },
            // Other deb822 actions on copyright paragraphs aren't common
            // enough to special-case; fall through to the generic path.
            _ => {
                return apply_generic_deb822_group(base, rel, group);
            }
        }
    }

    if any_change {
        std::fs::write(&abs, copyright.to_string())?;
    }
    Ok(any_change)
}

fn apply_generic_deb822_group(
    base: &Path,
    rel: &Path,
    group: &[&Action],
) -> Result<bool, FixerError> {
    use std::str::FromStr;

    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "deb822 action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let mut deb822 = deb822_lossless::Deb822::from_str(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse {}: {:?}", rel.display(), e)))?;

    let mut any_change = false;
    for action in group {
        let Action::Deb822(deb) = action else {
            unreachable!("apply_generic_deb822_group called with non-deb822 action");
        };
        match deb {
            Deb822Action::SetField {
                paragraph,
                field,
                value,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    return Err(FixerError::Other(format!(
                        "deb822 SetField on {}: no paragraph matching {:?}",
                        rel.display(),
                        paragraph
                    )));
                };
                if p.get(field).as_deref() == Some(value.as_str()) {
                    continue;
                }
                p.set(field, value);
                any_change = true;
            }
            Deb822Action::SetFieldWithIndent {
                paragraph,
                field,
                value,
                indent,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    return Err(FixerError::Other(format!(
                        "deb822 SetFieldWithIndent on {}: no paragraph matching {:?}",
                        rel.display(),
                        paragraph
                    )));
                };
                if p.get(field).as_deref() == Some(value.as_str()) {
                    continue;
                }
                p.set_with_indent_pattern(field, value, Some(&indent.to_deb822()), None);
                any_change = true;
            }
            Deb822Action::RemoveField {
                paragraph, field, ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if p.get(field).is_none() {
                    continue;
                }
                p.remove(field);
                any_change = true;
            }
            Deb822Action::RenameField {
                paragraph,
                from,
                to,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if p.rename(from, to) {
                    any_change = true;
                }
            }
            Deb822Action::RemoveParagraph { paragraph, .. } => {
                let Some(idx) = find_generic_paragraph_index(&deb822, paragraph)? else {
                    continue;
                };
                deb822.remove_paragraph(idx);
                any_change = true;
            }
            Deb822Action::AppendParagraph { fields, indent, .. } => {
                let mut p = deb822.add_paragraph();
                let pattern = indent.map(deb822_lossless::IndentPattern::Fixed);
                for (k, v) in fields {
                    p.set_with_indent_pattern(k, v, pattern.as_ref(), None);
                }
                any_change = true;
            }
            Deb822Action::NormalizeFieldSpacing {
                paragraph, field, ..
            } => {
                let Some(p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if let Some(mut entry) = p.get_entry(field) {
                    if entry.normalize_field_spacing() {
                        any_change = true;
                    }
                }
            }
            Deb822Action::DropRelation {
                paragraph,
                field,
                package,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if drop_relation_in_paragraph(&mut p, field, package) {
                    any_change = true;
                }
            }
            Deb822Action::ReplaceRelation {
                paragraph,
                field,
                from_package,
                to_entry,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if replace_relation_in_paragraph(&mut p, field, from_package, to_entry) {
                    any_change = true;
                }
            }
            Deb822Action::EnsureSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if ensure_substvar_in_paragraph(&mut p, field, substvar)? {
                    any_change = true;
                }
            }
            Deb822Action::DropSubstvar {
                paragraph,
                field,
                substvar,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if drop_substvar_in_paragraph(&mut p, field, substvar) {
                    any_change = true;
                }
            }
            Deb822Action::EnsureRelation {
                paragraph,
                field,
                entry,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if ensure_relation_in_paragraph(&mut p, field, entry)? {
                    any_change = true;
                }
            }
            Deb822Action::MoveRelation {
                paragraph,
                from_field,
                to_field,
                package,
                ..
            } => {
                let Some(mut p) = pick_generic_paragraph(&deb822, paragraph)? else {
                    continue;
                };
                if move_relation_in_paragraph(&mut p, from_field, to_field, package) {
                    any_change = true;
                }
            }
            Deb822Action::ReorderParagraphs {
                key_field, order, ..
            } => {
                if reorder_paragraphs(&mut deb822, key_field, order) {
                    any_change = true;
                }
            }
        }
    }

    if any_change {
        std::fs::write(&abs, deb822.to_string())?;
    }
    Ok(any_change)
}

/// Reorder a subset of paragraphs in `deb822`. Paragraphs whose
/// `key_field` value appears in `order` are moved into the order
/// specified, occupying the same positions originally held by
/// participating paragraphs. Returns true if any paragraph moved.
fn reorder_paragraphs(
    deb822: &mut deb822_lossless::Deb822,
    key_field: &str,
    order: &[String],
) -> bool {
    // Snapshot the current positions of paragraphs that have key_field,
    // along with their current key value.
    let participants: Vec<(usize, String)> = deb822
        .paragraphs()
        .enumerate()
        .filter_map(|(idx, p)| p.get(key_field).map(|v| (idx, v.to_string())))
        .collect();

    // Build the desired sequence of keys, restricted to those that
    // actually exist in the document, preserving the order given.
    let present: std::collections::HashSet<&str> =
        participants.iter().map(|(_, v)| v.as_str()).collect();
    let desired_keys: Vec<&str> = order
        .iter()
        .map(|s| s.as_str())
        .filter(|k| present.contains(k))
        .collect();

    if desired_keys.len() != participants.len() {
        // Some participating paragraphs aren't covered by `order`. We
        // could leave them in place, but the most straightforward
        // semantic is "only act when `order` covers all participants",
        // which the detector already arranges for. Treat this as a
        // no-op.
        return false;
    }

    // Walk participants in document order and move the one whose key
    // matches the desired key into the slot. We use `move_paragraph`
    // and re-snapshot positions after each move because moves shift
    // indices around.
    let mut changed = false;
    for (slot, want_key) in desired_keys.iter().enumerate() {
        // Re-snapshot.
        let participants: Vec<(usize, String)> = deb822
            .paragraphs()
            .enumerate()
            .filter_map(|(idx, p)| p.get(key_field).map(|v| (idx, v.to_string())))
            .collect();
        let dest_idx = participants[slot].0;
        // Find the current index of the paragraph that should be in
        // this slot.
        let Some(src_idx) = participants
            .iter()
            .find(|(_, v)| v == *want_key)
            .map(|(idx, _)| *idx)
        else {
            continue;
        };
        if src_idx == dest_idx {
            continue;
        }
        deb822.move_paragraph(src_idx, dest_idx);
        changed = true;
    }

    changed
}

/// Like [`pick_generic_paragraph`] but returns the paragraph's index. Used
/// for operations that need to address the paragraph in its parent
/// (e.g. removal).
fn find_generic_paragraph_index(
    deb822: &deb822_lossless::Deb822,
    selector: &ParagraphSelector,
) -> Result<Option<usize>, FixerError> {
    match selector {
        ParagraphSelector::CopyrightHeader => Ok(if deb822.paragraphs().next().is_some() {
            Some(0)
        } else {
            None
        }),
        ParagraphSelector::CopyrightFiles { glob } => Ok(deb822
            .paragraphs()
            .position(|p| p.get("Files").as_deref() == Some(glob.as_str()))),
        ParagraphSelector::Index { index } => Ok(if deb822.paragraphs().nth(*index).is_some() {
            Some(*index)
        } else {
            None
        }),
        ParagraphSelector::ByKey { field, value } => Ok(deb822
            .paragraphs()
            .position(|p| p.get(field).as_deref() == Some(value.as_str()))),
        ParagraphSelector::Source | ParagraphSelector::Binary { .. } => {
            Err(FixerError::Other(format!(
                "deb822 action: {:?} only applies to debian/control",
                selector
            )))
        }
    }
}

/// Pick a paragraph from a deb822 file using a generic-applicable selector.
///
/// Source/Binary selectors aren't accepted here — those go through the
/// typed control editor in [`apply_control_deb822_group`].
fn pick_generic_paragraph(
    deb822: &deb822_lossless::Deb822,
    selector: &ParagraphSelector,
) -> Result<Option<deb822_lossless::Paragraph>, FixerError> {
    match selector {
        ParagraphSelector::CopyrightHeader => Ok(deb822.paragraphs().next()),
        ParagraphSelector::CopyrightFiles { glob } => Ok(deb822
            .paragraphs()
            .find(|p| p.get("Files").as_deref() == Some(glob.as_str()))),
        ParagraphSelector::Index { index } => Ok(deb822.paragraphs().nth(*index)),
        ParagraphSelector::ByKey { field, value } => Ok(deb822
            .paragraphs()
            .find(|p| p.get(field).as_deref() == Some(value.as_str()))),
        ParagraphSelector::Source | ParagraphSelector::Binary { .. } => {
            Err(FixerError::Other(format!(
                "deb822 action: {:?} only applies to debian/control",
                selector
            )))
        }
    }
}

fn set_deb822_field(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    value: &str,
    indent: Option<&crate::diagnostic::IndentPattern>,
) -> Result<bool, FixerError> {
    // When `indent` is None we use Source::set / Binary::set on the typed
    // editor, which applies the canonical debian/control field ordering
    // (e.g. Priority lands after Section, before Description). When it's
    // Some we fall through to set_with_indent_pattern on the underlying
    // deb822 paragraph, which preserves position but skips the typed
    // editor's reordering — that's acceptable for fields like
    // Description that are already in canonical position when set.
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Err(FixerError::Other(
                    "deb822 SetField on Source: no source paragraph".into(),
                ));
            };
            if source.as_deb822().get(field).as_deref() == Some(value) {
                return Ok(false);
            }
            if let Some(pattern) = indent {
                source.as_mut_deb822().set_with_indent_pattern(
                    field,
                    value,
                    Some(&pattern.to_deb822()),
                    None,
                );
            } else {
                source.set(field, value);
            }
            Ok(true)
        }
        ParagraphSelector::Binary { package } => {
            let mut found = false;
            let mut changed = false;
            for mut binary in editor.binaries() {
                if binary.as_deb822().get("Package").as_deref() != Some(package.as_str()) {
                    continue;
                }
                found = true;
                if binary.as_deb822().get(field).as_deref() == Some(value) {
                    break;
                }
                if let Some(pattern) = indent {
                    binary.as_mut_deb822().set_with_indent_pattern(
                        field,
                        value,
                        Some(&pattern.to_deb822()),
                        None,
                    );
                } else {
                    binary.set(field, value);
                }
                changed = true;
                break;
            }
            if !found {
                return Err(FixerError::Other(format!(
                    "deb822 SetField on Binary({}): no such binary paragraph",
                    package
                )));
            }
            Ok(changed)
        }
        other => Err(FixerError::Other(format!(
            "deb822 SetField does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn remove_deb822_field(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            let p = source.as_mut_deb822();
            if p.get(field).is_none() {
                return Ok(false);
            }
            p.remove(field);
            Ok(true)
        }
        ParagraphSelector::Binary { package } => {
            let mut changed = false;
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(package.as_str()) {
                    continue;
                }
                if p.get(field).is_some() {
                    p.remove(field);
                    changed = true;
                }
                break;
            }
            Ok(changed)
        }
        other => Err(FixerError::Other(format!(
            "deb822 RemoveField does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn drop_relation_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    field: &str,
    package: &str,
) -> bool {
    use debian_control::lossless::relations::Relations;
    let Some(value) = p.get(field) else {
        return false;
    };
    let (mut relations, _errors) = Relations::parse_relaxed(&value, true);
    if !relations.drop_dependency(package) {
        return false;
    }
    let new_value = relations.to_string();
    if new_value.trim().is_empty() || relations.is_empty() {
        p.remove(field);
    } else {
        p.set(field, &new_value);
    }
    true
}

/// Replace the first relation entry that names `from_package` with the
/// parsed `to_entry`, preserving its position. If `to_entry` parses as a
/// relation whose package is already named elsewhere in the field, the
/// matching entry is dropped instead of replaced (keeps the field
/// duplicate-free).
fn replace_relation_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    field: &str,
    from_package: &str,
    to_entry: &str,
) -> bool {
    use debian_control::lossless::relations::{Entry, Relations};
    use std::str::FromStr;

    let Some(value) = p.get(field) else {
        return false;
    };
    let (mut relations, _errors) = Relations::parse_relaxed(&value, true);
    let Some((idx, _)) = relations.iter_relations_for(from_package).next() else {
        return false;
    };

    let Ok(new_entry) = Entry::from_str(to_entry) else {
        return false;
    };
    let new_name = new_entry
        .relations()
        .next()
        .and_then(|r| r.try_name())
        .unwrap_or_default();
    let new_already_present = !new_name.is_empty()
        && relations
            .iter_relations_for(&new_name)
            .any(|(other_idx, _)| other_idx != idx);

    if new_already_present {
        relations.drop_dependency(from_package);
    } else {
        relations.replace(idx, new_entry);
    }

    let new_value = relations.to_string();
    if new_value.trim().is_empty() || relations.is_empty() {
        p.remove(field);
    } else {
        p.set(field, &new_value);
    }
    true
}

/// Compute the post-edit relations for a substvar ensure: parse the
/// current field value, return the new relations if the substvar wasn't
/// already present (else `None`).
fn ensure_substvar_compute(
    current: Option<&str>,
    substvar: &str,
) -> Result<Option<debian_control::lossless::relations::Relations>, FixerError> {
    use debian_control::lossless::relations::Relations;
    let (mut relations, _errors) = Relations::parse_relaxed(current.unwrap_or_default(), true);
    let already_present = relations.substvars().any(|s| s == substvar);
    if already_present {
        return Ok(None);
    }
    relations
        .ensure_substvar(substvar)
        .map_err(FixerError::Other)?;
    Ok(Some(relations))
}

fn ensure_substvar_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    field: &str,
    substvar: &str,
) -> Result<bool, FixerError> {
    let current = p.get(field);
    let Some(new_relations) = ensure_substvar_compute(current.as_deref(), substvar)? else {
        return Ok(false);
    };
    p.set(field, &new_relations.to_string());
    Ok(true)
}

fn drop_substvar_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    field: &str,
    substvar: &str,
) -> bool {
    use debian_control::lossless::relations::Relations;
    let Some(value) = p.get(field) else {
        return false;
    };
    let (mut relations, _errors) = Relations::parse_relaxed(&value, true);
    if !relations.substvars().any(|s| s == substvar) {
        return false;
    }
    relations.drop_substvar(substvar);
    let new_value = relations.to_string();
    if new_value.trim().is_empty() || relations.is_empty() {
        p.remove(field);
    } else {
        p.set(field, &new_value);
    }
    true
}

fn drop_deb822_relation(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    package: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            Ok(drop_relation_in_paragraph(
                source.as_mut_deb822(),
                field,
                package,
            ))
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                return Ok(drop_relation_in_paragraph(p, field, package));
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 DropRelation does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn replace_deb822_relation(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    from_package: &str,
    to_entry: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            Ok(replace_relation_in_paragraph(
                source.as_mut_deb822(),
                field,
                from_package,
                to_entry,
            ))
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                return Ok(replace_relation_in_paragraph(
                    p,
                    field,
                    from_package,
                    to_entry,
                ));
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 ReplaceRelation does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn ensure_deb822_substvar(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    substvar: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            let current = source.as_deb822().get(field);
            let Some(new_relations) = ensure_substvar_compute(current.as_deref(), substvar)? else {
                return Ok(false);
            };
            match field {
                "Build-Depends" => source.set_build_depends(&new_relations),
                "Build-Depends-Indep" => source.set_build_depends_indep(&new_relations),
                "Build-Depends-Arch" => source.set_build_depends_arch(&new_relations),
                _ => {
                    source.set(field, &new_relations.to_string());
                }
            }
            Ok(true)
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                if binary.as_deb822().get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                let current = binary.as_deb822().get(field);
                let Some(new_relations) = ensure_substvar_compute(current.as_deref(), substvar)?
                else {
                    return Ok(false);
                };
                match field {
                    "Depends" => binary.set_depends(Some(&new_relations)),
                    "Recommends" => binary.set_recommends(Some(&new_relations)),
                    "Suggests" => binary.set_suggests(Some(&new_relations)),
                    "Pre-Depends" => binary.set_pre_depends(Some(&new_relations)),
                    "Conflicts" => binary.set_conflicts(Some(&new_relations)),
                    "Replaces" => binary.set_replaces(Some(&new_relations)),
                    "Provides" => binary.set_provides(Some(&new_relations)),
                    "Breaks" => binary.set_breaks(Some(&new_relations)),
                    "Built-Using" => binary.set_built_using(Some(&new_relations)),
                    "Static-Built-Using" => binary.set_static_built_using(Some(&new_relations)),
                    _ => {
                        binary.set(field, &new_relations.to_string());
                    }
                }
                return Ok(true);
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 EnsureSubstvar does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn ensure_relation_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    field: &str,
    entry: &str,
) -> Result<bool, FixerError> {
    let current = p.get(field);
    let Some(new_relations) = ensure_relation_compute(current.as_deref(), entry)? else {
        return Ok(false);
    };
    p.set(field, &new_relations.to_string());
    Ok(true)
}

/// Compute the post-edit relations for a field: parse the current field
/// value (if any), apply the requested ensure operation, and return both
/// the resulting relations and whether the field changed. Returns `None`
/// when there's no change to write.
fn ensure_relation_compute(
    current: Option<&str>,
    entry: &str,
) -> Result<Option<debian_control::lossless::relations::Relations>, FixerError> {
    use debian_control::lossless::relations::Relations;
    use std::str::FromStr;

    let requested_entry = debian_control::lossless::Entry::from_str(entry).map_err(|e| {
        FixerError::Other(format!("Failed to parse relation entry {:?}: {}", entry, e))
    })?;
    let Some(first) = requested_entry.relations().next() else {
        return Err(FixerError::Other(format!(
            "Relation entry {:?} has no relations",
            entry
        )));
    };
    let Some(name) = first.try_name() else {
        return Err(FixerError::Other(format!(
            "Relation entry {:?} has no package name",
            entry
        )));
    };
    let version = first.version();

    let (mut relations, _errors) = Relations::parse_relaxed(current.unwrap_or_default(), true);

    let changed = if let Some((constraint, ver)) = version {
        match constraint {
            debian_control::relations::VersionConstraint::Equal => {
                debian_analyzer::relations::ensure_exact_version(&mut relations, &name, &ver, None)
            }
            debian_control::relations::VersionConstraint::GreaterThanEqual => {
                let before = relations.to_string();
                relations.ensure_minimum_version(&name, &ver);
                relations.to_string() != before
            }
            other => {
                return Err(FixerError::Other(format!(
                    "EnsureRelation only supports `=` and `>=` version constraints, got {:?} in {:?}",
                    other, entry
                )));
            }
        }
    } else {
        // Pass the original entry string through verbatim so build-profile
        // suffixes like `pkg <!nocheck>` round-trip correctly via
        // Relation::simple's literal-name behaviour. The parsed `name` would
        // have stripped the suffix.
        let before = relations.to_string();
        debian_analyzer::relations::ensure_some_version(&mut relations, entry);
        relations.to_string() != before
    };

    Ok(if changed { Some(relations) } else { None })
}

/// Move the entry for `package` from `from_field` to `to_field` in
/// `paragraph`. Returns true if either field was modified.
fn move_relation_in_paragraph(
    p: &mut deb822_lossless::Paragraph,
    from_field: &str,
    to_field: &str,
    package: &str,
) -> bool {
    use debian_control::lossless::relations::Relations;

    let Some(from_value) = p.get(from_field) else {
        return false;
    };
    let (mut from_relations, _errors) = Relations::parse_relaxed(&from_value, true);
    let Ok((_pos, moved_entry)) = from_relations.get_relation(package) else {
        return false;
    };
    if !from_relations.drop_dependency(package) {
        return false;
    }

    // Update or remove the source field.
    if from_relations.is_empty() || from_relations.to_string().trim().is_empty() {
        p.remove(from_field);
    } else {
        p.set(from_field, &from_relations.to_string());
    }

    // Append (sorted) to the destination field.
    let to_value = p.get(to_field).unwrap_or_default();
    let (mut to_relations, _errors) = Relations::parse_relaxed(&to_value, true);
    to_relations.add_dependency(moved_entry, None);
    p.set(to_field, &to_relations.to_string());

    true
}

fn move_deb822_relation(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    from_field: &str,
    to_field: &str,
    package: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            Ok(move_relation_in_paragraph(
                source.as_mut_deb822(),
                from_field,
                to_field,
                package,
            ))
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                if binary.as_deb822().get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                return Ok(move_relation_in_paragraph(
                    binary.as_mut_deb822(),
                    from_field,
                    to_field,
                    package,
                ));
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 MoveRelation does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn ensure_deb822_relation(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    entry: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            // Read the current value via the typed accessor when available
            // so we can write back via a typed setter that places the
            // field canonically.
            let current = source.as_deb822().get(field);
            let Some(new_relations) = ensure_relation_compute(current.as_deref(), entry)? else {
                return Ok(false);
            };
            match field {
                "Build-Depends" => source.set_build_depends(&new_relations),
                "Build-Depends-Indep" => source.set_build_depends_indep(&new_relations),
                "Build-Depends-Arch" => source.set_build_depends_arch(&new_relations),
                _ => {
                    source.set(field, &new_relations.to_string());
                }
            }
            Ok(true)
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                if binary.as_deb822().get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                let current = binary.as_deb822().get(field);
                let Some(new_relations) = ensure_relation_compute(current.as_deref(), entry)?
                else {
                    return Ok(false);
                };
                match field {
                    "Depends" => binary.set_depends(Some(&new_relations)),
                    "Recommends" => binary.set_recommends(Some(&new_relations)),
                    "Suggests" => binary.set_suggests(Some(&new_relations)),
                    "Pre-Depends" => binary.set_pre_depends(Some(&new_relations)),
                    "Conflicts" => binary.set_conflicts(Some(&new_relations)),
                    "Replaces" => binary.set_replaces(Some(&new_relations)),
                    "Provides" => binary.set_provides(Some(&new_relations)),
                    "Breaks" => binary.set_breaks(Some(&new_relations)),
                    "Built-Using" => binary.set_built_using(Some(&new_relations)),
                    _ => {
                        binary.set(field, &new_relations.to_string());
                    }
                }
                return Ok(true);
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 EnsureRelation does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn drop_deb822_substvar(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
    substvar: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            Ok(drop_substvar_in_paragraph(
                source.as_mut_deb822(),
                field,
                substvar,
            ))
        }
        ParagraphSelector::Binary { package: pkg } => {
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(pkg.as_str()) {
                    continue;
                }
                return Ok(drop_substvar_in_paragraph(p, field, substvar));
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 DropSubstvar does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn normalize_deb822_field_spacing(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    field: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            let p = source.as_mut_deb822();
            let Some(mut entry) = p.get_entry(field) else {
                return Ok(false);
            };
            Ok(entry.normalize_field_spacing())
        }
        ParagraphSelector::Binary { package } => {
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(package.as_str()) {
                    continue;
                }
                let Some(mut entry) = p.get_entry(field) else {
                    return Ok(false);
                };
                return Ok(entry.normalize_field_spacing());
            }
            Ok(false)
        }
        other => Err(FixerError::Other(format!(
            "deb822 NormalizeFieldSpacing does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn rename_deb822_field(
    editor: &TemplatedControlEditor,
    paragraph: &ParagraphSelector,
    from: &str,
    to: &str,
) -> Result<bool, FixerError> {
    match paragraph {
        ParagraphSelector::Source => {
            let Some(mut source) = editor.source() else {
                return Ok(false);
            };
            // Paragraph::rename preserves the field's position.
            Ok(source.as_mut_deb822().rename(from, to))
        }
        ParagraphSelector::Binary { package } => {
            let mut changed = false;
            for mut binary in editor.binaries() {
                let p = binary.as_mut_deb822();
                if p.get("Package").as_deref() != Some(package.as_str()) {
                    continue;
                }
                changed = p.rename(from, to);
                break;
            }
            Ok(changed)
        }
        other => Err(FixerError::Other(format!(
            "deb822 RenameField does not support paragraph selector {:?}",
            other
        ))),
    }
}

fn apply_systemd_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    use std::str::FromStr;

    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "systemd action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let mut unit = systemd_unit_edit::SystemdUnit::from_str(&content).map_err(|e| {
        FixerError::Other(format!(
            "Failed to parse systemd unit {}: {:?}",
            rel.display(),
            e
        ))
    })?;

    let mut any_change = false;
    for action in group {
        let Action::Systemd(s) = action else {
            unreachable!("apply_systemd_group called with non-systemd action");
        };
        match s {
            SystemdAction::SetField {
                section,
                field,
                value,
                ..
            } => {
                let mut sec = match unit.get_section(section) {
                    Some(s) => s,
                    None => {
                        unit.add_section(section);
                        unit.get_section(section).expect("just added")
                    }
                };
                if sec.get_all(field).as_slice() == [value.clone()] {
                    continue;
                }
                sec.set(field, value);
                any_change = true;
            }
            SystemdAction::RemoveField { section, field, .. } => {
                let Some(mut sec) = unit.get_section(section) else {
                    continue;
                };
                if sec.get(field).is_none() {
                    continue;
                }
                sec.remove_all(field);
                any_change = true;
            }
            SystemdAction::RenameField {
                section, from, to, ..
            } => {
                let Some(mut sec) = unit.get_section(section) else {
                    continue;
                };
                let values = sec.get_all(from);
                if values.is_empty() {
                    continue;
                }
                sec.remove_all(from);
                for v in values {
                    sec.add(to, &v);
                }
                any_change = true;
            }
            SystemdAction::Add {
                section,
                field,
                value,
                ..
            } => {
                let mut sec = match unit.get_section(section) {
                    Some(s) => s,
                    None => {
                        unit.add_section(section);
                        unit.get_section(section).expect("just added")
                    }
                };
                if sec.get_all(field).contains(value) {
                    continue;
                }
                sec.add(field, value);
                any_change = true;
            }
            SystemdAction::RemoveValue {
                section,
                field,
                value,
                ..
            } => {
                let Some(mut sec) = unit.get_section(section) else {
                    continue;
                };
                // Multi-valued systemd fields (After=, Alias=, …) can mix
                // space-separated values on a single line and one-per-line
                // syntax. Check for membership across both forms before
                // calling remove_value, which handles the actual splitting.
                let present = sec
                    .get_all(field)
                    .iter()
                    .any(|line| line.split_whitespace().any(|v| v == value.as_str()));
                if !present {
                    continue;
                }
                sec.remove_value(field, value);
                any_change = true;
            }
        }
    }

    if any_change {
        std::fs::write(&abs, unit.text())?;
    }
    Ok(any_change)
}

fn apply_desktop_ini_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    use std::str::FromStr;

    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "desktop-ini action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let desktop = desktop_edit::Desktop::from_str(&content).map_err(|e| {
        FixerError::Other(format!(
            "Failed to parse desktop file {}: {:?}",
            rel.display(),
            e
        ))
    })?;

    let mut any_change = false;
    for action in group {
        let Action::DesktopIni(d) = action else {
            unreachable!("apply_desktop_ini_group called with non-desktop-ini action");
        };
        match d {
            DesktopIniAction::SetField {
                group: g,
                field,
                locale,
                value,
                ..
            } => {
                let Some(mut grp) = desktop.get_group(g) else {
                    return Err(FixerError::Other(format!(
                        "desktop-ini SetField on {}: no [{}] group",
                        rel.display(),
                        g
                    )));
                };
                match locale {
                    Some(loc) => {
                        if grp.get_locale(field, loc).as_deref() == Some(value.as_str()) {
                            continue;
                        }
                        grp.set_locale(field, loc, value);
                    }
                    None => {
                        if grp.get(field).as_deref() == Some(value.as_str()) {
                            continue;
                        }
                        grp.set(field, value);
                    }
                }
                any_change = true;
            }
            DesktopIniAction::RemoveField {
                group: g,
                field,
                locale,
                ..
            } => {
                let Some(mut grp) = desktop.get_group(g) else {
                    continue;
                };
                match locale {
                    Some(loc) => {
                        if grp.get_locale(field, loc).is_none() {
                            continue;
                        }
                        grp.remove_locale(field, loc);
                    }
                    None => {
                        if grp.get(field).is_none() {
                            continue;
                        }
                        grp.remove(field);
                    }
                }
                any_change = true;
            }
            DesktopIniAction::RemoveAll {
                group: g, field, ..
            } => {
                let Some(mut grp) = desktop.get_group(g) else {
                    continue;
                };
                if grp.get(field).is_none() && grp.get_all(field).is_empty() {
                    continue;
                }
                grp.remove_all(field);
                any_change = true;
            }
            DesktopIniAction::RenameField {
                group: g, from, to, ..
            } => {
                let Some(mut grp) = desktop.get_group(g) else {
                    continue;
                };
                let entries = grp.get_all(from);
                if entries.is_empty() {
                    continue;
                }
                grp.remove_all(from);
                for (locale, value) in entries {
                    match locale {
                        Some(loc) => grp.set_locale(to, &loc, &value),
                        None => grp.set(to, &value),
                    }
                }
                any_change = true;
            }
        }
    }

    if any_change {
        std::fs::write(&abs, desktop.to_string())?;
    }
    Ok(any_change)
}

fn apply_yaml_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    let file_existed = abs.exists();
    // YamlFile preserves file-level directives like `%YAML 1.1`; Document
    // alone discards them on round-trip. When the file doesn't exist
    // yet, start from an empty mapping document so YamlAction::SetField
    // / SetFieldOrdered actions can create the file.
    let (yaml_file, doc): (Option<yaml_edit::YamlFile>, yaml_edit::Document) = if file_existed {
        let yaml_file = yaml_edit::YamlFile::from_path(&abs).map_err(|e| {
            FixerError::Other(format!("Failed to open YAML {}: {}", rel.display(), e))
        })?;
        let Some(doc) = yaml_file.document() else {
            return Err(FixerError::Other(format!(
                "yaml action targets {}: no document",
                rel.display()
            )));
        };
        (Some(yaml_file), doc)
    } else {
        let new_mapping = yaml_edit::Mapping::new();
        let doc = yaml_edit::Document::from_mapping(new_mapping);
        (None, doc)
    };

    let mut any_change = false;
    for action in group {
        let Action::Yaml(yaml) = action else {
            unreachable!("apply_yaml_group called with non-yaml action");
        };
        match yaml {
            YamlAction::SetField {
                parent_path,
                key,
                value,
                ..
            } => {
                let Some(mapping) = navigate_yaml_mapping(&doc, parent_path)? else {
                    return Err(FixerError::Other(format!(
                        "yaml SetField on {}: path {:?} did not resolve to a mapping",
                        rel.display(),
                        parent_path
                    )));
                };
                if let Some(existing) = mapping.get(key.as_str()) {
                    if let yaml_edit::YamlNode::Scalar(scalar) = existing {
                        if scalar.as_string() == *value {
                            continue;
                        }
                    }
                }
                mapping.set(key.as_str(), value.as_str());
                any_change = true;
            }
            YamlAction::SetFieldOrdered {
                parent_path,
                key,
                value,
                field_order,
                ..
            } => {
                let Some(mapping) = navigate_yaml_mapping(&doc, parent_path)? else {
                    return Err(FixerError::Other(format!(
                        "yaml SetFieldOrdered on {}: path {:?} did not resolve to a mapping",
                        rel.display(),
                        parent_path
                    )));
                };
                if let Some(existing) = mapping.get(key.as_str()) {
                    if let yaml_edit::YamlNode::Scalar(scalar) = existing {
                        if scalar.as_string() == *value {
                            continue;
                        }
                    }
                }
                mapping.set_with_field_order(
                    key.as_str(),
                    value.as_str(),
                    field_order.iter().map(String::as_str),
                );
                any_change = true;
            }
            YamlAction::RemoveField {
                parent_path, key, ..
            } => {
                let Some(mapping) = navigate_yaml_mapping(&doc, parent_path)? else {
                    continue;
                };
                if !mapping.contains_key(key.as_str()) {
                    continue;
                }
                mapping.remove(key.as_str());
                any_change = true;
            }
            YamlAction::RenameField {
                parent_path,
                from,
                to,
                ..
            } => {
                let Some(mapping) = navigate_yaml_mapping(&doc, parent_path)? else {
                    continue;
                };
                if !mapping.contains_key(from.as_str()) {
                    continue;
                }
                if mapping.rename_key(from.as_str(), to.as_str()) {
                    any_change = true;
                }
            }
        }
    }

    if any_change {
        // Render from the YamlFile when it pre-existed (preserves
        // directives etc.); from the bare Document when we created the
        // file from scratch.
        let mut content = match &yaml_file {
            Some(yf) => yf.to_string(),
            None => doc.to_string(),
        };
        if !content.ends_with('\n') {
            content.push('\n');
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs, content)?;
    }
    Ok(any_change)
}

/// Walk down a YAML document along `path` and return the mapping that
/// path identifies. Returns `Ok(None)` if the path is well-formed but the
/// document doesn't contain that location (a missing key, an out-of-range
/// index). Returns `Err` if a path component is the wrong shape (e.g.
/// trying to index into a scalar).
fn navigate_yaml_mapping(
    doc: &yaml_edit::Document,
    path: &[YamlPathComponent],
) -> Result<Option<yaml_edit::Mapping>, FixerError> {
    let Some(mut mapping) = doc.as_mapping() else {
        return Ok(None);
    };
    for component in path {
        match component {
            YamlPathComponent::Key { key } => {
                let Some(next) = mapping.get_mapping(key.as_str()) else {
                    return Ok(None);
                };
                mapping = next;
            }
            YamlPathComponent::Index { .. } => {
                return Err(FixerError::Other(
                    "yaml action: sequence-index path components are not yet supported".into(),
                ));
            }
        }
    }
    Ok(Some(mapping))
}

fn apply_changelog_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    use debian_changelog::{iter_changes_by_author, ChangeLog};

    let abs = base.join(rel);
    let content = std::fs::read_to_string(&abs)?;
    let changelog = ChangeLog::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e)))?;

    let mut any_change = false;
    for action in group {
        let Action::Changelog(c) = action else {
            unreachable!("apply_changelog_group called with non-changelog action");
        };
        match c {
            ChangelogAction::SetEntryDate {
                version, rfc2822, ..
            } => {
                let target = changelog.iter().find(|e| {
                    e.version()
                        .map(|v| v.to_string() == *version)
                        .unwrap_or(false)
                });
                let Some(mut entry) = target else { continue };
                if entry.timestamp().as_deref() == Some(rfc2822.as_str()) {
                    continue;
                }
                entry.set_timestamp(rfc2822.clone());
                any_change = true;
            }
            ChangelogAction::ReplaceEntryChanges { version, lines, .. } => {
                let target = changelog.iter().find(|e| {
                    e.version()
                        .map(|v| v.to_string() == *version)
                        .unwrap_or(false)
                });
                let Some(entry) = target else {
                    // Entry has been renamed/removed since detection — treat
                    // as a no-op rather than erroring out.
                    continue;
                };
                let current: Vec<String> = entry.change_lines().collect();
                if current == *lines {
                    continue;
                }
                while entry.pop_change_line().is_some() {}
                for line in lines {
                    entry.append_change_line(line);
                }
                any_change = true;
            }
            ChangelogAction::RemoveBullet {
                version,
                author,
                text,
                occurrence,
                ..
            } => {
                // Walk the per-author bullet stream; skip the first
                // `occurrence` matches and remove the next one.
                let mut seen = 0usize;
                let mut removed = false;
                'outer: for change in iter_changes_by_author(&changelog) {
                    if change.version().map(|v| v.to_string()).as_deref() != Some(version.as_str())
                    {
                        continue;
                    }
                    for bullet in change.split_into_bullets() {
                        let bullet_author = bullet.author().map(|s| s.to_string());
                        let bullet_text = bullet.lines().join("\n");
                        if bullet_author == *author && bullet_text == *text {
                            if seen == *occurrence {
                                bullet.remove();
                                removed = true;
                                break 'outer;
                            }
                            seen += 1;
                        }
                    }
                }
                if removed {
                    any_change = true;
                }
            }
            ChangelogAction::ReplaceBullet {
                version,
                author,
                text,
                occurrence,
                new_lines,
                ..
            } => {
                let mut seen = 0usize;
                let mut replaced = false;
                'outer: for change in iter_changes_by_author(&changelog) {
                    if change.version().map(|v| v.to_string()).as_deref() != Some(version.as_str())
                    {
                        continue;
                    }
                    for bullet in change.split_into_bullets() {
                        let bullet_author = bullet.author().map(|s| s.to_string());
                        let bullet_text = bullet.lines().join("\n");
                        if bullet_author == *author && bullet_text == *text {
                            if seen == *occurrence {
                                let new_text = new_lines.join("\n");
                                if new_text == bullet_text {
                                    break 'outer;
                                }
                                let new_refs: Vec<&str> =
                                    new_lines.iter().map(|s| s.as_str()).collect();
                                bullet.replace_with(new_refs);
                                replaced = true;
                                break 'outer;
                            }
                            seen += 1;
                        }
                    }
                }
                if replaced {
                    any_change = true;
                }
            }
            ChangelogAction::SetEntryVersion {
                version,
                new_version,
                ..
            } => {
                use std::str::FromStr;
                let parsed_new = debversion::Version::from_str(new_version).map_err(|e| {
                    FixerError::Other(format!(
                        "Invalid new version {:?} in SetEntryVersion: {}",
                        new_version, e
                    ))
                })?;
                let mut updated = false;
                for mut entry in changelog.iter() {
                    let Some(entry_version) = entry.version() else {
                        continue;
                    };
                    if entry_version.to_string() != *version {
                        continue;
                    }
                    if entry_version == parsed_new {
                        break;
                    }
                    entry.set_version(&parsed_new);
                    updated = true;
                    break;
                }
                if updated {
                    any_change = true;
                }
            }
        }
    }

    if any_change {
        std::fs::write(&abs, changelog.to_string())?;
    }
    Ok(any_change)
}

fn apply_watch_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "watch action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let watch_file = debian_watch::parse::parse(&content)
        .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e)))?;

    let mut any_change = false;
    for action in group {
        let Action::Watch(w) = action else {
            unreachable!("apply_watch_group called with non-watch action");
        };
        match w {
            WatchAction::SetEntryMatchingPattern {
                url, new_pattern, ..
            } => {
                let mut found = false;
                for mut entry in watch_file.entries() {
                    if &entry.url() != url {
                        continue;
                    }
                    found = true;
                    let current = entry.matching_pattern().unwrap_or_default();
                    if &current == new_pattern {
                        break;
                    }
                    entry.set_matching_pattern(new_pattern);
                    any_change = true;
                    break;
                }
                if !found {
                    // Idempotency: if the entry was already updated by a
                    // sibling action and the URL key has shifted, the
                    // detector's snapshot is stale; treat as a no-op.
                    continue;
                }
            }
            WatchAction::RemoveEntryOption { url, option, .. } => {
                for mut entry in watch_file.entries() {
                    if &entry.url() != url {
                        continue;
                    }
                    if entry.get_option(option).is_none() {
                        break;
                    }
                    match &mut entry {
                        debian_watch::parse::ParsedEntry::LineBased(e) => e.del_opt_str(option),
                        debian_watch::parse::ParsedEntry::Deb822(e) => e.delete_option_str(option),
                    }
                    any_change = true;
                    break;
                }
            }
            WatchAction::SetEntryOption {
                url, option, value, ..
            } => {
                for mut entry in watch_file.entries() {
                    if &entry.url() != url {
                        continue;
                    }
                    if entry.get_option(option).as_deref() == Some(value.as_str()) {
                        break;
                    }
                    match &mut entry {
                        debian_watch::parse::ParsedEntry::LineBased(e) => e.set_opt(option, value),
                        debian_watch::parse::ParsedEntry::Deb822(e) => {
                            e.set_option_str(option, value)
                        }
                    }
                    any_change = true;
                    break;
                }
            }
            WatchAction::SetEntryUrl { url, new_url, .. } => {
                for mut entry in watch_file.entries() {
                    if &entry.url() != url {
                        continue;
                    }
                    if &entry.url() == new_url {
                        break;
                    }
                    entry.set_url(new_url);
                    any_change = true;
                    break;
                }
            }
            WatchAction::ConvertEntryToTemplate { url, .. } => {
                for mut entry in watch_file.entries() {
                    if &entry.url() != url {
                        continue;
                    }
                    // Templates are a v5 (deb822) feature only.
                    if let debian_watch::parse::ParsedEntry::Deb822(e) = &mut entry {
                        if e.try_convert_to_template().is_some() {
                            any_change = true;
                        }
                    }
                    break;
                }
            }
        }
    }

    if any_change {
        std::fs::write(&abs, watch_file.to_string())?;
    }
    Ok(any_change)
}

fn apply_makefile_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "makefile action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let mut makefile = makefile_lossless::Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e)))?;

    let mut any_change = false;
    let mut rules: Vec<_> = makefile.rules().collect();
    for action in group {
        let Action::Makefile(m) = action else {
            unreachable!("apply_makefile_group called with non-makefile action");
        };
        match m {
            MakefileAction::ReplaceRecipe {
                target,
                recipe,
                new_recipe,
                ..
            } => {
                for rule in &mut rules {
                    if !rule.targets().any(|t| &t == target) {
                        continue;
                    }
                    let recipe_index = rule
                        .recipe_nodes()
                        .position(|r| r.text() == recipe.as_str());
                    let Some(idx) = recipe_index else {
                        continue;
                    };
                    let replacement =
                        if new_recipe.chars().next().is_some_and(|c| c.is_whitespace()) {
                            new_recipe.clone()
                        } else {
                            let indent: String =
                                recipe.chars().take_while(|c| c.is_whitespace()).collect();
                            format!("{}{}", indent, new_recipe)
                        };
                    if rule.replace_command(idx, &replacement) {
                        any_change = true;
                    }
                    break;
                }
            }
            MakefileAction::RemoveRecipe { target, recipe, .. } => {
                for rule in &mut rules {
                    if !rule.targets().any(|t| &t == target) {
                        continue;
                    }
                    let recipe_index = rule
                        .recipe_nodes()
                        .position(|r| r.text() == recipe.as_str());
                    let Some(idx) = recipe_index else {
                        continue;
                    };
                    if rule.remove_command(idx) {
                        any_change = true;
                    }
                    break;
                }
            }
            MakefileAction::SetVariable { name, value, .. } => {
                if let Some(mut var) = makefile
                    .variable_definitions()
                    .find(|v| v.name().as_deref() == Some(name.as_str()))
                {
                    if var.raw_value().as_deref().map(str::trim) != Some(value.as_str()) {
                        var.set_value(value);
                        any_change = true;
                    }
                }
            }
            MakefileAction::SetVariableOperator { name, operator, .. } => {
                if let Some(mut var) = makefile
                    .variable_definitions()
                    .find(|v| v.name().as_deref() == Some(name.as_str()))
                {
                    if var.assignment_operator().as_deref() != Some(operator.as_str()) {
                        var.set_assignment_operator(operator);
                        any_change = true;
                    }
                }
            }
            MakefileAction::RemoveVariable { name, .. } => {
                if let Some(mut var) = makefile
                    .variable_definitions()
                    .find(|v| v.name().as_deref() == Some(name.as_str()))
                {
                    var.remove();
                    any_change = true;
                }
            }
            MakefileAction::RemoveRule { target, .. } => {
                let idx = makefile
                    .rules()
                    .position(|r| r.targets().any(|t| t.trim() == target.as_str()));
                if let Some(idx) = idx {
                    makefile
                        .remove_rule(idx)
                        .map_err(|e| FixerError::Other(format!("Failed to remove rule: {}", e)))?;
                    rules = makefile.rules().collect();
                    any_change = true;
                }
            }
            MakefileAction::RemovePhonyTarget { target, .. } => {
                let removed = makefile.remove_phony_target(target).map_err(|e| {
                    FixerError::Other(format!("Failed to remove phony target: {}", e))
                })?;
                if removed {
                    rules = makefile.rules().collect();
                    any_change = true;
                }
            }
            MakefileAction::RenameRuleTarget {
                from_target,
                to_target,
                ..
            } => {
                for rule in &mut rules {
                    if !rule.targets().any(|t| t.trim() == from_target.as_str()) {
                        continue;
                    }
                    let renamed = rule.rename_target(from_target, to_target).map_err(|e| {
                        FixerError::Other(format!("Failed to rename target: {}", e))
                    })?;
                    if renamed {
                        any_change = true;
                    }
                    break;
                }
            }
            MakefileAction::AddRule {
                target,
                prerequisites,
                ..
            } => {
                let mut rule = makefile.add_rule(target);
                for prereq in prerequisites {
                    rule.add_prerequisite(prereq).map_err(|e| {
                        FixerError::Other(format!("Failed to add prerequisite: {}", e))
                    })?;
                }
                rules = makefile.rules().collect();
                any_change = true;
            }
            MakefileAction::AddPhonyTarget { target, .. } => {
                let already = makefile
                    .find_rule_by_target(".PHONY")
                    .is_some_and(|r| r.prerequisites().any(|p| &p == target));
                if already {
                    continue;
                }
                makefile
                    .add_phony_target(target)
                    .map_err(|e| FixerError::Other(format!("Failed to add phony target: {}", e)))?;
                rules = makefile.rules().collect();
                any_change = true;
            }
            MakefileAction::AddInclude { path, .. } => {
                if makefile.included_files().any(|f| &f == path) {
                    continue;
                }
                // String-level insertion: place the include directive
                // after the leading shebang/comment/blank-line block, so
                // the shebang stays first. Splicing into the syntax tree
                // via `add_include`/`insert_include` doesn't preserve that
                // visual separation.
                let current = makefile.code();
                let mut split = 0usize;
                let mut saw_non_comment = false;
                for line in current.split_inclusive('\n') {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    let is_shebang = split == 0 && trimmed.starts_with("#!");
                    let is_comment = trimmed.starts_with('#') && !is_shebang;
                    let is_blank = trimmed.is_empty();
                    if is_shebang || is_comment || is_blank {
                        split += line.len();
                        continue;
                    }
                    saw_non_comment = true;
                    break;
                }
                let insertion = format!("include {}\n", path);
                let new_content = if saw_non_comment {
                    format!("{}{}{}", &current[..split], insertion, &current[split..])
                } else {
                    format!("{}{}", &current[..split], insertion)
                };
                makefile = makefile_lossless::Makefile::read_relaxed(new_content.as_bytes())
                    .map_err(|e| {
                        FixerError::Other(format!("Failed to reparse {}: {}", rel.display(), e))
                    })?;
                rules = makefile.rules().collect();
                any_change = true;
            }
            MakefileAction::ReplaceVariableWithInclude { name, path, .. } => {
                if makefile.included_files().any(|f| &f == path) {
                    if let Some(mut var) = makefile
                        .variable_definitions()
                        .find(|v| v.name().as_deref() == Some(name.as_str()))
                    {
                        var.remove();
                        rules = makefile.rules().collect();
                        any_change = true;
                    }
                    continue;
                }
                let temp = format!("include {}\n", path)
                    .parse::<makefile_lossless::Makefile>()
                    .map_err(|e| {
                        FixerError::Other(format!("Failed to build include node: {}", e))
                    })?;
                let include = temp.includes().next().ok_or_else(|| {
                    FixerError::Other("Failed to extract include from temp makefile".into())
                })?;
                let items: Vec<_> = makefile.items().collect();
                let mut found = false;
                for mut item in items {
                    if let makefile_lossless::MakefileItem::Variable(var) = &item {
                        if var.name().as_deref() == Some(name.as_str()) {
                            item.replace(makefile_lossless::MakefileItem::Include(include.clone()))
                                .map_err(|e| {
                                    FixerError::Other(format!("Failed to replace variable: {}", e))
                                })?;
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    rules = makefile.rules().collect();
                    any_change = true;
                }
            }
            MakefileAction::InsertIncludeBeforeVariable {
                path,
                before_variable,
                ..
            } => {
                if makefile.included_files().any(|f| &f == path) {
                    continue;
                }
                let temp = format!("include {}\n", path)
                    .parse::<makefile_lossless::Makefile>()
                    .map_err(|e| {
                        FixerError::Other(format!("Failed to build include node: {}", e))
                    })?;
                let include = temp.includes().next().ok_or_else(|| {
                    FixerError::Other("Failed to extract include from temp makefile".into())
                })?;
                let items: Vec<_> = makefile.items().collect();
                let mut inserted = false;
                for mut item in items {
                    if let makefile_lossless::MakefileItem::Variable(var) = &item {
                        if var.name().as_deref() == Some(before_variable.as_str()) {
                            item.insert_before(makefile_lossless::MakefileItem::Include(
                                include.clone(),
                            ))
                            .map_err(|e| {
                                FixerError::Other(format!("Failed to insert include: {}", e))
                            })?;
                            inserted = true;
                            break;
                        }
                    }
                }
                if inserted {
                    rules = makefile.rules().collect();
                    any_change = true;
                }
            }
        }
    }

    if any_change {
        std::fs::write(&abs, makefile.code())?;
    }
    Ok(any_change)
}

/// Find the byte offset where the patch's diff body starts. The header
/// runs from the start of the file up to (but not including) the first
/// `---`, `diff `, or `Index:` line.
fn dep3_header_end(content: &str) -> usize {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.starts_with("---")
            || trimmed.starts_with("diff ")
            || trimmed.starts_with("Index:")
        {
            return offset;
        }
        offset += line.len();
    }
    content.len()
}

fn apply_dep3_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "DEP-3 action targets missing file {}",
            rel.display()
        )));
    }
    let content = std::fs::read_to_string(&abs)?;
    let header_end = dep3_header_end(&content);
    let header_str = &content[..header_end];
    let body = &content[header_end..];

    let mut header: dep3::lossless::PatchHeader = header_str
        .parse()
        .map_err(|e| FixerError::Other(format!("Failed to parse DEP-3 header: {:?}", e)))?;
    let original = header.to_string();

    for action in group {
        let Action::Dep3(d) = action else {
            unreachable!("apply_dep3_group called with non-DEP-3 action");
        };
        let para = header.as_deb822_mut();
        match d {
            Dep3Action::SetField { field, value, .. } => {
                para.set(field, value);
            }
            Dep3Action::RemoveField { field, .. } => {
                para.remove(field);
            }
            Dep3Action::RenameField {
                from_field,
                to_field,
                ..
            } => {
                let Some(value) = para.get(from_field) else {
                    continue;
                };
                para.remove(from_field);
                para.set(to_field, &value);
            }
        }
    }

    if header.to_string() == original {
        return Ok(false);
    }
    let new_content = format!("{}{}", header, body);
    std::fs::write(&abs, new_content)?;
    Ok(true)
}

fn override_line_matches(
    line: &lintian_overrides::OverrideLine,
    selector: &OverrideLineSelector,
) -> bool {
    if line.is_comment() || line.is_empty() {
        return false;
    }
    let Some(tag) = line.tag() else {
        return false;
    };
    if tag.text() != selector.tag {
        return false;
    }
    let line_info = line.info();
    let line_info_norm = line_info.as_deref().map(str::trim).unwrap_or("");
    let selector_info = selector.info.as_deref().unwrap_or("");
    if line_info_norm != selector_info {
        return false;
    }
    let line_pkg = line.package_spec().and_then(|s| s.package_name());
    if line_pkg.as_deref() != selector.package.as_deref() {
        return false;
    }
    true
}

fn apply_lintian_overrides_group(
    base: &Path,
    rel: &Path,
    group: &[&Action],
) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    if !abs.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&abs)?;
    let parsed = lintian_overrides::LintianOverrides::parse(&content);
    let mut overrides = parsed.ok().map_err(|errs| {
        FixerError::Other(format!(
            "Failed to parse {}: {}",
            rel.display(),
            errs.join(", ")
        ))
    })?;

    let original = overrides.text();
    for action in group {
        let Action::LintianOverrides(a) = action else {
            unreachable!("apply_lintian_overrides_group called with non-overrides action");
        };
        match a {
            LintianOverridesAction::DropLine { selector, .. } => {
                let mut dropped = false;
                overrides = lintian_overrides::filter_overrides(&overrides, |line| {
                    if dropped {
                        return true;
                    }
                    if override_line_matches(line, selector) {
                        dropped = true;
                        false
                    } else {
                        true
                    }
                });
            }
            LintianOverridesAction::RenameTag {
                from_tag, to_tag, ..
            } => {
                overrides = lintian_overrides::rename_tags(&overrides, |tag| {
                    if tag == from_tag {
                        Some(to_tag.clone())
                    } else {
                        None
                    }
                });
            }
            LintianOverridesAction::SetLineInfo {
                selector, new_info, ..
            } => {
                let mut applied = false;
                overrides = lintian_overrides::map_overrides(&overrides, |line| {
                    if applied {
                        return None;
                    }
                    if !override_line_matches(line, selector) {
                        return None;
                    }
                    applied = true;
                    let package_spec = line.package_spec();
                    let package = package_spec.as_ref().and_then(|s| s.package_name());
                    let package_type = package_spec.as_ref().and_then(|s| s.package_type());
                    let tag = line.tag()?.text().to_string();
                    let info = if new_info.is_empty() {
                        None
                    } else {
                        Some(new_info.clone())
                    };
                    Some((package, package_type, tag, info))
                });
            }
        }
    }

    let new_content = overrides.text();
    if new_content == original {
        return Ok(false);
    }
    // If the file has nothing meaningful left, remove it. The driver
    // already handles partial deletes for us — emit the same behaviour
    // the legacy fixers had.
    let has_content = overrides.lines().any(|l| !l.is_comment() && !l.is_empty());
    if !has_content {
        std::fs::remove_file(&abs)?;
    } else {
        std::fs::write(&abs, new_content)?;
    }
    Ok(true)
}

fn apply_filesystem_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    let abs = base.join(rel);
    let mut any_change = false;
    for action in group {
        let Action::Filesystem(fs) = action else {
            unreachable!("apply_filesystem_group called with non-filesystem action");
        };
        match fs {
            FilesystemAction::SetMode { mode, .. } => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(*mode);
                    let current = std::fs::metadata(&abs)?.permissions();
                    if current.mode() & 0o7777 == *mode & 0o7777 {
                        continue;
                    }
                    std::fs::set_permissions(&abs, perms)?;
                    any_change = true;
                }
                #[cfg(not(unix))]
                {
                    let _ = mode;
                    return Err(FixerError::Other(
                        "FilesystemAction::SetMode is only supported on Unix".into(),
                    ));
                }
            }
            FilesystemAction::Delete { .. } => match std::fs::remove_file(&abs) {
                Ok(()) => any_change = true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(FixerError::Io(e)),
            },
            FilesystemAction::Rename { to, .. } => {
                let to_abs = base.join(to);
                if !abs.exists() {
                    // Source already gone — treat as a no-op rather than
                    // an error, mirroring the other actions' idempotency.
                    continue;
                }
                if let Some(parent) = to_abs.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&abs, &to_abs)?;
                any_change = true;
            }
            FilesystemAction::RemoveDirIfEmpty { .. } => match std::fs::remove_dir(&abs) {
                Ok(()) => any_change = true,
                Err(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        || e.kind() == std::io::ErrorKind::DirectoryNotEmpty =>
                {
                    // The dir is gone or still has siblings — neither is
                    // a fixer error.
                }
                Err(e) => return Err(FixerError::Io(e)),
            },
            FilesystemAction::Write { content, .. } => {
                let prev = std::fs::read(&abs).ok();
                if prev.as_deref() == Some(content.as_slice()) {
                    continue;
                }
                if let Some(parent) = abs.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&abs, content)?;
                any_change = true;
            }
            FilesystemAction::ReplaceText {
                range, replacement, ..
            } => {
                let mut content = std::fs::read_to_string(&abs)?;
                if range.start > range.end || range.end > content.len() {
                    return Err(FixerError::Other(format!(
                        "ReplaceText range {}..{} out of bounds for {} (len {})",
                        range.start,
                        range.end,
                        rel.display(),
                        content.len()
                    )));
                }
                if !content.is_char_boundary(range.start) || !content.is_char_boundary(range.end) {
                    return Err(FixerError::Other(format!(
                        "ReplaceText range {}..{} not on char boundaries in {}",
                        range.start,
                        range.end,
                        rel.display()
                    )));
                }
                if &content[range.start..range.end] == replacement {
                    continue;
                }
                content.replace_range(range.start..range.end, replacement);
                std::fs::write(&abs, content)?;
                any_change = true;
            }
            FilesystemAction::Substitute { from, to, .. } => {
                if from.is_empty() {
                    return Err(FixerError::Other(format!(
                        "FilesystemAction::Substitute on {} has empty `from`",
                        rel.display()
                    )));
                }
                let content = std::fs::read_to_string(&abs)?;
                if !content.contains(from.as_str()) {
                    continue;
                }
                let new_content = content.replace(from.as_str(), to.as_str());
                if new_content == content {
                    continue;
                }
                std::fs::write(&abs, new_content)?;
                any_change = true;
            }
        }
    }
    Ok(any_change)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::TextRange;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn deb822_set_field_on_source() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "Source: foo\n\nPackage: foo\n").unwrap();

        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Priority".into(),
            value: "optional".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        let after = fs::read_to_string(debian.join("control")).unwrap();
        assert_eq!(after, "Source: foo\nPriority: optional\n\nPackage: foo\n");
    }

    #[test]
    fn deb822_set_field_idempotent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\nPriority: optional\n\nPackage: foo\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Priority".into(),
            value: "optional".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn deb822_normalize_field_spacing_on_binary() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: bar\nRecommends:  baz\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::NormalizeFieldSpacing {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "bar".into(),
            },
            field: "Recommends".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\n\nPackage: bar\nRecommends: baz\n",
        );
    }

    #[test]
    fn deb822_drop_relation_removes_named_dep() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: build-essential, debhelper-compat (= 13)\n\nPackage: foo\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::DropRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            package: "build-essential".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_drop_relation_idempotent_when_absent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\nBuild-Depends: debhelper\n\nPackage: foo\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::DropRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            package: "build-essential".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn deb822_drop_relation_removes_field_when_empty() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends: cdbs\n\nPackage: foo\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::DropRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            package: "cdbs".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_ensure_substvar_appends() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: foo\nDepends: ${shlibs:Depends}\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::EnsureSubstvar {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "foo".into(),
            },
            field: "Depends".into(),
            substvar: "${misc:Depends}".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\n\nPackage: foo\nDepends: ${shlibs:Depends}, ${misc:Depends}\n",
        );
    }

    #[test]
    fn deb822_ensure_substvar_idempotent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\n\nPackage: foo\nDepends: ${misc:Depends}\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::EnsureSubstvar {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "foo".into(),
            },
            field: "Depends".into(),
            substvar: "${misc:Depends}".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn deb822_ensure_relation_appends_unversioned() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends:\n debhelper,\n pkg-config\n\nPackage: foo\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::EnsureRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "python3-setuptools".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\nBuild-Depends:\n debhelper,\n pkg-config,\n python3-setuptools\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_ensure_relation_idempotent_when_present() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\nBuild-Depends: python3-setuptools, debhelper\n\nPackage: foo\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::EnsureRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "python3-setuptools".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn deb822_ensure_relation_versioned_creates_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "Source: foo\n\nPackage: foo\n").unwrap();

        let action = Action::Deb822(Deb822Action::EnsureRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            entry: "debhelper-compat (= 13)".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 13)\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_move_relation_between_fields() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\nBuild-Depends-Indep: debhelper-compat (= 12)\nBuild-Depends: python3\n\nPackage: foo\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::MoveRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            from_field: "Build-Depends-Indep".into(),
            to_field: "Build-Depends".into(),
            package: "debhelper-compat".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\nBuild-Depends: debhelper-compat (= 12), python3\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_move_relation_idempotent_when_absent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\nBuild-Depends: python3\n\nPackage: foo\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::MoveRelation {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Source,
            from_field: "Build-Depends-Indep".into(),
            to_field: "Build-Depends".into(),
            package: "debhelper-compat".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn watch_set_entry_matching_pattern_updates() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("watch"),
            "version=4\nhttps://github.com/foo/bar/tags .*/archive/(.*)\\.tar\\.gz\n",
        )
        .unwrap();

        let action = Action::Watch(WatchAction::SetEntryMatchingPattern {
            file: PathBuf::from("debian/watch"),
            url: "https://github.com/foo/bar/tags".into(),
            new_pattern: ".*/archive/refs/tags/(.*)\\.tar\\.gz".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("watch")).unwrap(),
            "version=4\nhttps://github.com/foo/bar/tags .*/archive/refs/tags/(.*)\\.tar\\.gz\n",
        );
    }

    #[test]
    fn watch_set_entry_matching_pattern_idempotent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial =
            "version=4\nhttps://github.com/foo/bar/tags .*/archive/refs/tags/(.*)\\.tar\\.gz\n";
        fs::write(debian.join("watch"), initial).unwrap();

        let action = Action::Watch(WatchAction::SetEntryMatchingPattern {
            file: PathBuf::from("debian/watch"),
            url: "https://github.com/foo/bar/tags".into(),
            new_pattern: ".*/archive/refs/tags/(.*)\\.tar\\.gz".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("watch")).unwrap(), initial);
    }

    #[test]
    fn deb822_drop_substvar_removes_substvar() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: foo\nBuilt-Using: ${misc:Built-Using}\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::DropSubstvar {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "foo".into(),
            },
            field: "Built-Using".into(),
            substvar: "${misc:Built-Using}".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(changed);
        assert_eq!(
            fs::read_to_string(debian.join("control")).unwrap(),
            "Source: foo\n\nPackage: foo\n",
        );
    }

    #[test]
    fn deb822_normalize_field_spacing_idempotent() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        let initial = "Source: foo\n\nPackage: bar\nRecommends: baz\n";
        fs::write(debian.join("control"), initial).unwrap();

        let action = Action::Deb822(Deb822Action::NormalizeFieldSpacing {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "bar".into(),
            },
            field: "Recommends".into(),
        });
        let changed = apply_action(tmp.path(), &action).unwrap();
        assert!(!changed);
        assert_eq!(fs::read_to_string(debian.join("control")).unwrap(), initial);
    }

    #[test]
    fn deb822_remove_then_set_grouped() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("control"),
            "Source: foo\n\nPackage: foo\nPriority: optional\n\nPackage: foo-doc\nPriority: optional\n",
        )
        .unwrap();

        let actions = vec![
            Action::Deb822(Deb822Action::RemoveField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: "foo".into(),
                },
                field: "Priority".into(),
            }),
            Action::Deb822(Deb822Action::RemoveField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Binary {
                    package: "foo-doc".into(),
                },
                field: "Priority".into(),
            }),
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Priority".into(),
                value: "optional".into(),
            }),
        ];
        let changed = apply_actions(tmp.path(), &actions).unwrap();
        assert_eq!(changed, vec![PathBuf::from("debian/control")]);
        let after = fs::read_to_string(debian.join("control")).unwrap();
        assert_eq!(
            after,
            "Source: foo\nPriority: optional\n\nPackage: foo\n\nPackage: foo-doc\n"
        );
    }

    #[test]
    fn deb822_unknown_binary_errors() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "Source: foo\n\nPackage: foo\n").unwrap();

        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/control"),
            paragraph: ParagraphSelector::Binary {
                package: "missing".into(),
            },
            field: "Priority".into(),
            value: "optional".into(),
        });
        let err = apply_action(tmp.path(), &action).unwrap_err();
        assert!(matches!(err, FixerError::Other(_)));
    }

    #[test]
    #[cfg(unix)]
    fn filesystem_set_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("script");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let action = Action::Filesystem(FilesystemAction::SetMode {
            file: PathBuf::from("script"),
            mode: 0o755,
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o755);
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn filesystem_write_creates_dirs() {
        let tmp = TempDir::new().unwrap();
        let action = Action::Filesystem(FilesystemAction::Write {
            file: PathBuf::from("debian/source/format"),
            content: b"3.0 (quilt)\n".to_vec(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/format")).unwrap(),
            "3.0 (quilt)\n"
        );
    }

    #[test]
    fn filesystem_delete_missing_is_noop() {
        let tmp = TempDir::new().unwrap();
        let action = Action::Filesystem(FilesystemAction::Delete {
            file: PathBuf::from("nope"),
        });
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn filesystem_rename_creates_dirs_and_atomic() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("debian")).unwrap();
        fs::write(tmp.path().join("debian/source.lintian-overrides"), "x\n").unwrap();

        let action = Action::Filesystem(FilesystemAction::Rename {
            file: PathBuf::from("debian/source.lintian-overrides"),
            to: PathBuf::from("debian/source/lintian-overrides"),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        assert!(!tmp.path().join("debian/source.lintian-overrides").exists());
        assert_eq!(
            fs::read_to_string(tmp.path().join("debian/source/lintian-overrides")).unwrap(),
            "x\n"
        );
        // Idempotent: source already gone is a no-op.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn filesystem_replace_text() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("file.txt");
        fs::write(&path, "hello world").unwrap();

        let action = Action::Filesystem(FilesystemAction::ReplaceText {
            file: PathBuf::from("file.txt"),
            range: TextRange { start: 6, end: 11 },
            replacement: "rust".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello rust");
    }

    #[test]
    fn mixed_kinds_for_same_file_errors() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(debian.join("control"), "Source: foo\n\nPackage: foo\n").unwrap();

        let actions = vec![
            Action::Deb822(Deb822Action::SetField {
                file: PathBuf::from("debian/control"),
                paragraph: ParagraphSelector::Source,
                field: "Priority".into(),
                value: "optional".into(),
            }),
            Action::Filesystem(FilesystemAction::Delete {
                file: PathBuf::from("debian/control"),
            }),
        ];
        let err = apply_actions(tmp.path(), &actions).unwrap_err();
        assert!(matches!(err, FixerError::Other(_)));
    }

    #[test]
    fn systemd_set_field_replaces_value() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.service");
        fs::write(
            &path,
            "[Service]\nPIDFile=/var/run/foo.pid\nExecStart=/bin/foo\n",
        )
        .unwrap();

        let action = Action::Systemd(SystemdAction::SetField {
            file: PathBuf::from("foo.service"),
            section: "Service".into(),
            field: "PIDFile".into(),
            value: "/run/foo.pid".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[Service]\nPIDFile=/run/foo.pid\nExecStart=/bin/foo\n",
        );
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn systemd_rename_field_preserves_multivalued() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.service");
        fs::write(&path, "[Unit]\nBindTo=a.service\nBindTo=b.service\n").unwrap();

        let action = Action::Systemd(SystemdAction::RenameField {
            file: PathBuf::from("foo.service"),
            section: "Unit".into(),
            from: "BindTo".into(),
            to: "BindsTo".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("BindsTo=a.service"));
        assert!(after.contains("BindsTo=b.service"));
        assert!(!after.contains("BindTo="));
    }

    #[test]
    fn systemd_remove_value_keeps_siblings() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.service");
        fs::write(&path, "[Unit]\nAfter=syslog.target\nAfter=network.target\n").unwrap();

        let action = Action::Systemd(SystemdAction::RemoveValue {
            file: PathBuf::from("foo.service"),
            section: "Unit".into(),
            field: "After".into(),
            value: "syslog.target".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("syslog.target"));
        assert!(after.contains("After=network.target"));
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn systemd_add_appends_value() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.service");
        fs::write(&path, "[Unit]\nAfter=network.target\n").unwrap();

        let action = Action::Systemd(SystemdAction::Add {
            file: PathBuf::from("foo.service"),
            section: "Unit".into(),
            field: "Before".into(),
            value: "shutdown.target".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Before=shutdown.target"));
        // Idempotent: adding the same value twice is a no-op.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn desktop_ini_set_field() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.desktop");
        fs::write(&path, "[Desktop Entry]\nName=Foo\nType=Application\n").unwrap();

        let action = Action::DesktopIni(DesktopIniAction::SetField {
            file: PathBuf::from("foo.desktop"),
            group: "Desktop Entry".into(),
            field: "Name".into(),
            locale: None,
            value: "Bar".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Name=Bar"));
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn desktop_ini_remove_field() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nName=Foo\nEncoding=UTF-8\nType=Application\n",
        )
        .unwrap();

        let action = Action::DesktopIni(DesktopIniAction::RemoveField {
            file: PathBuf::from("foo.desktop"),
            group: "Desktop Entry".into(),
            field: "Encoding".into(),
            locale: None,
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("Encoding="));
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn generic_deb822_set_header_field() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: http://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightHeader,
            field: "Format".into(),
            value: "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        assert_eq!(
            fs::read_to_string(debian.join("copyright")).unwrap(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+\n",
        );
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn generic_deb822_select_files_glob() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("copyright"),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2024 Foo\nLicense: GPL-2+\n\nFiles: docs/*\nCopyright: 2024 Bar\nLicense: GFDL-1.3+\n",
        )
        .unwrap();

        let action = Action::Deb822(Deb822Action::SetField {
            file: PathBuf::from("debian/copyright"),
            paragraph: ParagraphSelector::CopyrightFiles {
                glob: "docs/*".into(),
            },
            field: "License".into(),
            value: "GFDL-1.3+-or-later".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(debian.join("copyright")).unwrap();
        assert!(after.contains("Files: docs/*\nCopyright: 2024 Bar\nLicense: GFDL-1.3+-or-later"));
        // The other Files paragraph was not touched.
        assert!(after.contains("Files: *\nCopyright: 2024 Foo\nLicense: GPL-2+"));
    }

    #[test]
    fn yaml_set_field_top_level() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian/upstream");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("metadata");
        fs::write(&path, "Name: foo\nRepository: https://example.org/foo\n").unwrap();

        let action = Action::Yaml(YamlAction::SetField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: vec![],
            key: "Name".into(),
            value: "Foo".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Name: Foo"));
        assert!(after.contains("Repository: https://example.org/foo"));
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn yaml_remove_field_top_level() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian/upstream");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("metadata");
        fs::write(
            &path,
            "Name: foo\nObsolete-Field: nothing\nRepository: https://example.org/foo\n",
        )
        .unwrap();

        let action = Action::Yaml(YamlAction::RemoveField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: vec![],
            key: "Obsolete-Field".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("Obsolete-Field"));
        assert!(after.contains("Name: foo"));
        assert!(after.contains("Repository: https://example.org/foo"));
        // Idempotent.
        assert!(!apply_action(tmp.path(), &action).unwrap());
    }

    #[test]
    fn yaml_rename_field_preserves_position() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian/upstream");
        fs::create_dir_all(&debian).unwrap();
        let path = debian.join("metadata");
        fs::write(
            &path,
            "Name: foo\nRepo: https://example.org/foo\nBug-Database: https://example.org/foo/issues\n",
        )
        .unwrap();

        let action = Action::Yaml(YamlAction::RenameField {
            file: PathBuf::from("debian/upstream/metadata"),
            parent_path: vec![],
            from: "Repo".into(),
            to: "Repository".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Repository: https://example.org/foo"));
        assert!(!after.contains("Repo: "));
        // Bug-Database stays after Repository (position preserved).
        let repo_pos = after.find("Repository").unwrap();
        let bugdb_pos = after.find("Bug-Database").unwrap();
        assert!(repo_pos < bugdb_pos);
    }

    #[test]
    fn desktop_ini_set_locale_keeps_unlocalised() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.desktop");
        fs::write(&path, "[Desktop Entry]\nName=Foo\nType=Application\n").unwrap();

        let action = Action::DesktopIni(DesktopIniAction::SetField {
            file: PathBuf::from("foo.desktop"),
            group: "Desktop Entry".into(),
            field: "Name".into(),
            locale: Some("de".into()),
            value: "Fooey".into(),
        });
        assert!(apply_action(tmp.path(), &action).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("Name=Foo"));
        assert!(after.contains("Name[de]=Fooey"));
    }
}
