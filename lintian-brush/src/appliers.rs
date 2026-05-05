//! Apply [`Action`]s to a working tree.
//!
//! See `doc/detector-action-split.md` for the design rationale.
//!
//! Actions for the same file are batched into a single editor session so a
//! detector that emits e.g. one `RemoveField` per binary plus a `SetField`
//! on the source produces a single rewrite of `debian/control`.

use crate::diagnostic::{
    Action, ChangelogAction, Deb822Action, DesktopIniAction, FilesystemAction, ParagraphSelector,
    SystemdAction, YamlAction, YamlPathComponent,
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
            | Deb822Action::RemoveField { file, .. }
            | Deb822Action::RenameField { file, .. }
            | Deb822Action::RemoveParagraph { file, .. }
            | Deb822Action::AppendParagraph { file, .. } => file,
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
            | YamlAction::RemoveField { file, .. }
            | YamlAction::RenameField { file, .. } => file,
        },
        Action::Changelog(a) => match a {
            ChangelogAction::ReplaceEntryChanges { file, .. }
            | ChangelogAction::SetEntryDate { file, .. }
            | ChangelogAction::RemoveBullet { file, .. }
            | ChangelogAction::ReplaceBullet { file, .. } => file,
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
        Action::Filesystem(_) => apply_filesystem_group(base, rel, group),
    }
}

fn apply_deb822_group(base: &Path, rel: &Path, group: &[&Action]) -> Result<bool, FixerError> {
    // Selectors are tagged with the file family they belong to. We dispatch
    // on the first selector in the group: Source/Binary go through the
    // typed control editor (which applies canonical field ordering on
    // insert); CopyrightHeader/CopyrightFiles go through the generic
    // deb822 editor; Index/ByKey work on either and use the generic path.
    // AppendParagraph carries no selector and always uses the generic
    // path.
    let first = first_selector(group);
    let use_control_editor = matches!(
        first,
        Some(ParagraphSelector::Source | ParagraphSelector::Binary { .. })
    );
    if use_control_editor {
        apply_control_deb822_group(base, rel, group)
    } else {
        apply_generic_deb822_group(base, rel, group)
    }
}

fn first_selector<'a>(group: &'a [&'a Action]) -> Option<&'a ParagraphSelector> {
    for action in group {
        let Action::Deb822(deb) = action else {
            continue;
        };
        return match deb {
            Deb822Action::SetField { paragraph, .. }
            | Deb822Action::RemoveField { paragraph, .. }
            | Deb822Action::RenameField { paragraph, .. }
            | Deb822Action::RemoveParagraph { paragraph, .. } => Some(paragraph),
            Deb822Action::AppendParagraph { .. } => None,
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
    let editor = TemplatedControlEditor::open(&abs)?;
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
                if set_deb822_field(&editor, paragraph, field, value)? {
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
                return Err(FixerError::Other(format!(
                    "deb822 RemoveParagraph not supported on debian/control via the typed editor (selector: {:?})",
                    paragraph
                )));
            }
            Deb822Action::AppendParagraph { .. } => {
                return Err(FixerError::Other(
                    "deb822 AppendParagraph not supported on debian/control via the typed editor"
                        .into(),
                ));
            }
        }
    }

    if any_change {
        editor.commit()?;
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
        }
    }

    if any_change {
        std::fs::write(&abs, deb822.to_string())?;
    }
    Ok(any_change)
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
) -> Result<bool, FixerError> {
    // Source::set / Binary::set apply the canonical debian/control field
    // ordering, so a newly-introduced field lands at a sensible position
    // (e.g. Priority after Section, before Description).
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
            source.set(field, value);
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
                binary.set(field, value);
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
    if !abs.exists() {
        return Err(FixerError::Other(format!(
            "yaml action targets missing file {}",
            rel.display()
        )));
    }
    // YamlFile preserves file-level directives like `%YAML 1.1`; Document
    // alone discards them on round-trip.
    let yaml_file = yaml_edit::YamlFile::from_path(&abs)
        .map_err(|e| FixerError::Other(format!("Failed to open YAML {}: {}", rel.display(), e)))?;
    let Some(doc) = yaml_file.document() else {
        return Err(FixerError::Other(format!(
            "yaml action targets {}: no document",
            rel.display()
        )));
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
        let mut content = yaml_file.to_string();
        if !content.ends_with('\n') {
            content.push('\n');
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
        }
    }

    if any_change {
        std::fs::write(&abs, changelog.to_string())?;
    }
    Ok(any_change)
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
