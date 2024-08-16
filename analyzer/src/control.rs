use crate::editor::{Editor, EditorError, FsEditor};
use crate::relations::{ensure_relation, is_relation_implied};
use std::path::{Path, PathBuf};

/// Format a description based on summary and long description lines.
pub fn format_description(summary: &str, long_description: Vec<&str>) -> String {
    let mut ret = summary.to_string() + "\n";
    for line in long_description {
        ret.push(' ');
        ret.push_str(line);
        ret.push('\n');
    }
    ret
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_format_description() {
        let summary = "Summary";
        let long_description = vec!["Long", "Description"];
        let expected = "Summary\n Long\n Description\n";
        assert_eq!(
            super::format_description(summary, long_description),
            expected
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
enum TemplateType {
    Rules,
    Gnome,
    Postgresql,
    Directory,
    Cdbs,
    Debcargo,
}

#[derive(Debug)]
enum TemplateExpansionError {
    Failed(String),
    ExpandCommandMissing(String),
    UnknownTemplating(PathBuf, Option<PathBuf>),
    Conflict(ChangeConflict),
}

impl From<EditorError> for TemplateExpansionError {
    fn from(e: EditorError) -> Self {
        match e {
            EditorError::IoError(e) => TemplateExpansionError::Failed(format!("IO error: {}", e)),
            EditorError::BrzError(e) => TemplateExpansionError::Failed(format!("Bzr error: {}", e)),
            EditorError::GeneratedFile(p, _e) => TemplateExpansionError::UnknownTemplating(p, None),
            EditorError::FormattingUnpreservable(p, _e) => {
                TemplateExpansionError::UnknownTemplating(p, None)
            }
        }
    }
}

impl From<ChangeConflict> for TemplateExpansionError {
    fn from(e: ChangeConflict) -> Self {
        TemplateExpansionError::Conflict(e)
    }
}

impl std::fmt::Display for TemplateExpansionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TemplateExpansionError::Failed(s) => write!(f, "Failed: {}", s),
            TemplateExpansionError::ExpandCommandMissing(s) => {
                write!(f, "Command not found: {}", s)
            }
            TemplateExpansionError::UnknownTemplating(p1, p2) => {
                if let Some(p2) = p2 {
                    write!(
                        f,
                        "Unknown templating: {} -> {}",
                        p1.display(),
                        p2.display()
                    )
                } else {
                    write!(f, "Unknown templating: {}", p1.display())
                }
            }
            TemplateExpansionError::Conflict(c) => write!(f, "Conflict: {}", c),
        }
    }
}

impl std::error::Error for TemplateExpansionError {}

/// Run the dh_gnome_clean command.
///
/// This needs to do some post-hoc cleaning, since dh_gnome_clean writes various debhelper log
/// files that should not be checked in.
///
/// # Arguments
/// * `path` - Path to run dh_gnome_clean in
fn dh_gnome_clean(path: &std::path::Path) -> Result<(), TemplateExpansionError> {
    for n in std::fs::read_dir(path.join("debian")).unwrap() {
        if let Ok(entry) = n {
            if entry
                .file_name()
                .to_string_lossy()
                .ends_with(".debhelper.log")
            {
                return Err(TemplateExpansionError::Failed(
                    "pre-existing .debhelper.log files".to_string(),
                ));
            }
        }
    }

    if !path.join("debian/changelog").exists() {
        return Err(TemplateExpansionError::Failed(
            "no changelog file".to_string(),
        ));
    }

    let result = std::process::Command::new("dh_gnome_clean")
        .current_dir(path)
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(TemplateExpansionError::Failed(stderr.to_string()));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(TemplateExpansionError::ExpandCommandMissing(
                "dh_gnome_clean".to_string(),
            ));
        }
        Err(e) => {
            return Err(TemplateExpansionError::Failed(e.to_string()));
        }
    }

    for n in std::fs::read_dir(path.join("debian")).unwrap() {
        if let Ok(entry) = n {
            if entry
                .file_name()
                .to_string_lossy()
                .ends_with(".debhelper.log")
            {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
    }

    Ok(())
}

/// Run the 'pg_buildext updatecontrol' command.
///
/// # Arguments
/// * `path` - Path to run pg_buildext updatecontrol in
fn pg_buildext_updatecontrol(path: &std::path::Path) -> Result<(), TemplateExpansionError> {
    let result = std::process::Command::new("pg_buildext")
        .arg("updatecontrol")
        .current_dir(path)
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(TemplateExpansionError::Failed(stderr.to_string()));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(TemplateExpansionError::ExpandCommandMissing(
                "pg_buildext".to_string(),
            ));
        }
        Err(e) => {
            return Err(TemplateExpansionError::Failed(e.to_string()));
        }
    }
    Ok(())
}

fn set_mtime<P: AsRef<Path>>(path: P, mtime: std::time::SystemTime) -> nix::Result<()> {
    use nix::sys::stat::utimes;
    use nix::sys::time::TimeVal;

    let duration = mtime.duration_since(std::time::UNIX_EPOCH).unwrap();

    let seconds = duration.as_secs() as i64;
    let nanos = duration.subsec_nanos() as i64;

    let tv = TimeVal::new(seconds, nanos);

    utimes(path.as_ref(), &tv, &tv)
}

fn expand_control_template(
    template_path: &std::path::Path,
    path: &std::path::Path,
    template_type: TemplateType,
) -> Result<(), TemplateExpansionError> {
    let package_root = path.parent().unwrap().parent().unwrap();
    match template_type {
        TemplateType::Rules => {
            let path_time = match std::fs::metadata(path) {
                Ok(metadata) => Some(metadata.modified().unwrap()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => panic!("Failed to get mtime of {}: {}", path.display(), e),
            };
            while let Ok(metadata) = std::fs::metadata(template_path) {
                if Some(metadata.modified().unwrap()) == path_time {
                    // Wait until mtime has changed, so that make knows to regenerate.
                    set_mtime(template_path, std::time::SystemTime::now()).unwrap();
                } else {
                    break;
                }
            }
            let result = std::process::Command::new("./debian/rules")
                .arg("debian/control")
                .output();

            match result {
                Ok(output) => {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        Err(TemplateExpansionError::Failed(stderr.to_string()))
                    } else {
                        Ok(())
                    }
                }
                Err(e) => Err(TemplateExpansionError::Failed(e.to_string())),
            }
        }
        TemplateType::Gnome => dh_gnome_clean(package_root),
        TemplateType::Postgresql => pg_buildext_updatecontrol(package_root),
        TemplateType::Cdbs => unreachable!(),
        TemplateType::Debcargo => unreachable!(),
        TemplateType::Directory => Err(TemplateExpansionError::UnknownTemplating(
            path.to_path_buf(),
            Some(template_path.to_path_buf()),
        )),
    }
}

#[derive(Debug, Clone)]
struct Deb822Changes(
    std::collections::HashMap<(String, String), Vec<(String, Option<String>, Option<String>)>>,
);

fn update_control_template(
    template_path: &std::path::Path,
    path: &std::path::Path,
    changes: Deb822Changes,
    expand_template: bool,
) -> Result<bool, TemplateExpansionError> {
    let template_type = guess_template_type(template_path, Some(path.parent().unwrap()));

    match template_type {
        Some(TemplateType::Directory) => {
            // We can't handle these yet
            return Err(TemplateExpansionError::UnknownTemplating(
                path.to_path_buf(),
                Some(template_path.to_path_buf()),
            ));
        }
        None => {
            return Err(TemplateExpansionError::UnknownTemplating(
                path.to_path_buf(),
                Some(template_path.to_path_buf()),
            ));
        }
        _ => {}
    }

    let mut editor = FsEditor::<deb822_lossless::Deb822>::new(template_path, false, false).unwrap();

    let resolve_conflict = match template_type {
        Some(TemplateType::Cdbs) => Some(resolve_cdbs_template as ResolveDeb822Conflict),
        _ => None,
    };

    apply_changes(&mut editor, changes.clone(), resolve_conflict)?;

    if !editor.has_changed() {
        // A bit odd, since there were changes to the output file. Anyway.
        return Ok(false);
    }

    editor.commit()?;

    if expand_template {
        match template_type {
            Some(TemplateType::Cdbs) => {
                let mut editor =
                    FsEditor::<deb822_lossless::Deb822>::new(path, true, false).unwrap();
                apply_changes(&mut editor, changes, None)?;
            }
            _ => {
                expand_control_template(template_path, path, template_type.unwrap())?;
            }
        }
    }

    Ok(true)
}

#[derive(Debug)]
pub struct ChangeConflict {
    para_key: (String, String),
    field: String,
    actual_old_value: Option<String>,
    template_old_value: Option<String>,
    actual_new_value: Option<String>,
}

impl std::fmt::Display for ChangeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}/{}: {} -> {} (template: {})",
            self.para_key.0,
            self.para_key.1,
            self.actual_old_value.as_deref().unwrap_or(""),
            self.actual_new_value.as_deref().unwrap_or(""),
            self.template_old_value.as_deref().unwrap_or("")
        )
    }
}

impl std::error::Error for ChangeConflict {}

type ResolveDeb822Conflict = fn(
    para_key: (&str, &str),
    field: &str,
    actual_old_value: Option<&str>,
    template_old_value: Option<&str>,
    actual_new_value: Option<&str>,
) -> Result<Option<String>, ChangeConflict>;

fn resolve_cdbs_template(
    para_key: (&str, &str),
    field: &str,
    actual_old_value: Option<&str>,
    template_old_value: Option<&str>,
    actual_new_value: Option<&str>,
) -> Result<Option<String>, ChangeConflict> {
    if para_key.0 == "Source"
        && field == "Build-Depends"
        && template_old_value.is_some()
        && actual_old_value.is_some()
        && actual_new_value.is_some()
    {
        if actual_new_value
            .unwrap()
            .contains(actual_old_value.unwrap())
        {
            // We're simply adding to the existing list
            return Ok(Some(
                actual_new_value
                    .unwrap()
                    .replace(actual_old_value.unwrap(), template_old_value.unwrap()),
            ));
        } else {
            let old_rels: debian_control::relations::Relations =
                actual_old_value.unwrap().parse().unwrap();
            let new_rels: debian_control::relations::Relations =
                actual_new_value.unwrap().parse().unwrap();
            let mut ret: debian_control::relations::Relations =
                template_old_value.unwrap().parse().unwrap();
            for v in new_rels.entries() {
                if old_rels.entries().any(|r| is_relation_implied(&v, &r)) {
                    continue;
                }
                ensure_relation(&mut ret, v);
            }
            return Ok(Some(ret.to_string()));
        }
    }
    Err(ChangeConflict {
        para_key: (para_key.0.to_string(), para_key.1.to_string()),
        field: field.to_string(),
        actual_old_value: actual_old_value.map(|v| v.to_string()),
        template_old_value: template_old_value.map(|v| v.to_string()),
        actual_new_value: actual_new_value.map(|s| s.to_string()),
    })
}

/// Guess the type for a control template.
///
/// # Arguments
/// * `template_path` - Path to the control template
/// * `debian_path` - Path to the debian directory
///
/// # Returns
/// Template type; None if unknown
pub fn guess_template_type(
    template_path: &std::path::Path,
    debian_path: Option<&std::path::Path>,
) -> Option<TemplateType> {
    // TODO(jelmer): This should use a proper make file parser of some sort..
    if let Some(debian_path) = debian_path {
        match std::fs::read(debian_path.join("rules")) {
            Ok(file) => {
                for line in file.split(|&c| c == b'\n') {
                    if line.starts_with(b"debian/control:") {
                        return Some(TemplateType::Rules);
                    }
                    if line.starts_with(b"debian/%: debian/%.in") {
                        return Some(TemplateType::Rules);
                    }
                    if line.starts_with(b"include /usr/share/blends-dev/rules") {
                        return Some(TemplateType::Rules);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!(
                "Failed to read {}: {}",
                debian_path.join("rules").display(),
                e
            ),
        }
    }
    match std::fs::read(template_path) {
        Ok(template) => {
            let template_str = std::str::from_utf8(&template).unwrap();
            if template_str.contains("@GNOME_TEAM@") {
                return Some(TemplateType::Gnome);
            }
            if template_str.contains("PGVERSION") {
                return Some(TemplateType::Postgresql);
            }
            if template_str.contains("@cdbs@") {
                return Some(TemplateType::Cdbs);
            }

            let control = debian_control::Control::read_relaxed(std::io::Cursor::new(&template))
                .unwrap()
                .0;

            let build_depends = control.source().and_then(|s| s.build_depends());

            if build_depends.iter().any(|d| {
                d.entries()
                    .any(|e| e.relations().any(|r| r.name() == "pkg-gnome-tools"))
            }) {
                return Some(TemplateType::Gnome);
            }

            if build_depends.iter().any(|d| {
                d.entries()
                    .any(|e| e.relations().any(|r| r.name() == "postgresql"))
            }) {
                return Some(TemplateType::Cdbs);
            }
        }
        Err(_) if template_path.is_dir() => {
            return Some(TemplateType::Directory);
        }
        Err(e) => panic!("Failed to read {}: {}", template_path.display(), e),
    }
    if let Some(debian_path) = debian_path {
        if debian_path.join("debcargo.toml").exists() {
            return Some(TemplateType::Debcargo);
        }
    }
    None
}

/// Apply a set of changes to this deb822 instance.
///
/// # Arguments
/// * `changes` - Changes to apply
/// * `resolve_conflict` - Callback to resolve conflicts
pub fn apply_changes(
    deb822: &mut deb822_lossless::Deb822,
    mut changes: Deb822Changes,
    resolve_conflict: Option<ResolveDeb822Conflict>,
) -> Result<(), ChangeConflict> {
    fn default_resolve_conflict(
        para_key: (&str, &str),
        field: &str,
        actual_old_value: Option<&str>,
        template_old_value: Option<&str>,
        actual_new_value: Option<&str>,
    ) -> Result<Option<String>, ChangeConflict> {
        Err(ChangeConflict {
            para_key: (para_key.0.to_string(), para_key.1.to_string()),
            field: field.to_string(),
            actual_old_value: actual_old_value.map(|v| v.to_string()),
            template_old_value: template_old_value.map(|v| v.to_string()),
            actual_new_value: actual_new_value.map(|s| s.to_string()),
        })
    }

    let resolve_conflict = resolve_conflict.unwrap_or(default_resolve_conflict);

    for mut paragraph in deb822.paragraphs() {
        for item in paragraph.items().collect::<Vec<_>>() {
            for (key, old_value, mut new_value) in changes.0.remove(&item).unwrap_or_default() {
                if paragraph.get(&key) != old_value {
                    new_value = resolve_conflict(
                        (&item.0, &item.1),
                        &key,
                        old_value.as_deref(),
                        paragraph.get(&key).as_deref(),
                        new_value.as_deref(),
                    )?;
                }
                if let Some(new_value) = new_value.as_ref() {
                    paragraph.insert(&key, new_value);
                } else {
                    paragraph.remove(&key);
                }
            }
        }
    }
    // Add any new paragraphs that weren't processed earlier
    for (key, p) in changes.0.drain() {
        let mut paragraph = deb822.add_paragraph();
        for (field, old_value, mut new_value) in p {
            new_value = resolve_conflict(
                (&key.0, &key.1),
                &field,
                old_value.as_deref(),
                None,
                new_value.as_deref(),
            )?;
            if let Some(new_value) = new_value {
                paragraph.insert(&field, &new_value);
            }
        }
    }
    Ok(())
}
