use crate::editor::{Editor, EditorError, FsEditor, GeneratedFile};
use crate::relations::{ensure_relation, is_relation_implied};
use deb822_lossless::Paragraph;
use debian_control::relations::Relations;
use std::ops::{Deref, DerefMut};
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
    let nanos = duration.subsec_nanos();

    let tv = TimeVal::new(seconds, nanos as i32);

    utimes(path.as_ref(), &tv, &tv)
}

/// Expand a control template.
///
/// # Arguments
/// * `template_path` - Path to the control template
/// * `path` - Path to the control file
/// * `template_type` - Type of the template
///
/// # Returns
/// Ok if the template was successfully expanded
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

impl Deb822Changes {
    fn new() -> Self {
        Self(std::collections::HashMap::new())
    }

    fn insert(
        &mut self,
        para_key: (String, String),
        field: String,
        old_value: Option<String>,
        new_value: Option<String>,
    ) {
        self.0
            .entry(para_key)
            .or_insert_with(Vec::new)
            .push((field, old_value, new_value));
    }
}

// Update a control file template based on changes to the file itself.
//
// # Arguments
// * `template_path` - Path to the control template
// * `path` - Path to the control file
// * `changes` - Changes to apply
// * `expand_template` - Whether to expand the template after updating it
//
// # Returns
// Ok if the template was successfully updated
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

    match editor.commit() {
        Ok(_) => {}
        Err(e) => return Err(TemplateExpansionError::Failed(e.to_string())),
    }

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

#[derive(Debug, PartialEq, Eq)]
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
            let old_rels: Relations = actual_old_value.unwrap().parse().unwrap();
            let new_rels: Relations = actual_new_value.unwrap().parse().unwrap();
            let template_old_value = template_old_value.unwrap();
            let (mut ret, errors) = Relations::parse_relaxed(template_old_value, true);
            if !errors.is_empty() {
                log::debug!("Errors parsing template value: {:?}", errors);
            }
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
                    .any(|e| e.relations().any(|r| r.name() == "gnome-pkg-tools"))
            }) {
                return Some(TemplateType::Gnome);
            }

            if build_depends.iter().any(|d| {
                d.entries()
                    .any(|e| e.relations().any(|r| r.name() == "cdbs"))
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
            if old_value.is_some() {
                new_value = resolve_conflict(
                    (&key.0, &key.1),
                    &field,
                    old_value.as_deref(),
                    paragraph.get(&field).as_deref(),
                    new_value.as_deref(),
                )?;
            }
            if let Some(new_value) = new_value {
                paragraph.insert(&field, &new_value);
            }
        }
    }
    Ok(())
}

fn find_template_path(path: &Path) -> Option<PathBuf> {
    for ext in &["in", "m4"] {
        let template_path = path.with_extension(ext);
        if template_path.exists() {
            return Some(template_path);
        }
    }
    None
}

pub struct FsControlEditor {
    primary: FsEditor<deb822_lossless::Deb822>,
    path: PathBuf,
    template_only: bool,
}

impl Deref for FsControlEditor {
    type Target = deb822_lossless::Deb822;

    fn deref(&self) -> &Self::Target {
        &self.primary
    }
}

impl DerefMut for FsControlEditor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.primary
    }
}

impl FsControlEditor {
    pub fn new<P: AsRef<Path>>(control_path: P) -> Result<Self, EditorError> {
        let path = control_path.as_ref();
        let mut template_only = false;
        let primary;
        if !path.exists() {
            let template_path = if let Some(p) = find_template_path(&path) {
                p
            } else {
                return Err(EditorError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No control file or template found",
                )));
            };
            template_only = true;
            let template_type = guess_template_type(&template_path, Some(path.parent().unwrap()));
            if template_type.is_none() {
                return Err(EditorError::GeneratedFile(
                    path.to_path_buf(),
                    GeneratedFile {
                        template_path: Some(template_path),
                        template_type: None,
                    },
                ));
            }
            match expand_control_template(&template_path, &path, template_type.unwrap()) {
                Ok(_) => {}
                Err(e) => return Err(EditorError::TemplateError(template_path, e.to_string())),
            }
            primary = FsEditor::<deb822_lossless::Deb822>::new(&path, true, false)?;
        } else {
            primary = FsEditor::<deb822_lossless::Deb822>::new(&path, true, false)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            primary,
            template_only,
        })
    }

    /// Return a dictionary describing the changes since the base.
    ///
    /// # Returns
    /// A dictionary mapping tuples of (kind, name) to list of (field_name, old_value, new_value)
    pub fn changes(&self) -> Deb822Changes {
        let orig = deb822_lossless::Deb822::read_relaxed(self.primary.orig_content().unwrap())
            .unwrap()
            .0;
        let mut changes = Deb822Changes::new();

        fn by_key(
            ps: impl Iterator<Item = Paragraph>,
        ) -> std::collections::HashMap<(String, String), Paragraph> {
            let mut ret = std::collections::HashMap::new();
            for p in ps {
                if let Some(s) = p.get("Source") {
                    ret.insert(("Source".to_string(), s), p);
                } else if let Some(s) = p.get("Package") {
                    ret.insert(("Package".to_string(), s), p);
                } else {
                    let k = p.items().next().unwrap().clone();
                    ret.insert(k, p);
                }
            }
            ret
        }

        let orig_by_key = by_key(orig.paragraphs());
        let new_by_key = by_key(self.paragraphs());
        let keys = orig_by_key
            .keys()
            .chain(new_by_key.keys())
            .collect::<std::collections::HashSet<_>>();
        for key in keys {
            let old = orig_by_key.get(key);
            let new = new_by_key.get(key);
            if old == new {
                continue;
            }
            let fields = std::collections::HashSet::<String>::from_iter(
                old.iter()
                    .flat_map(|p| p.keys())
                    .chain(new.iter().flat_map(|p| p.keys())),
            );
            for field in &fields {
                let old_val = old.and_then(|x| x.get(&field));
                let new_val = new.and_then(|x| x.get(&field));
                if old_val != new_val {
                    changes.insert(key.clone(), field.to_string(), old_val, new_val);
                }
            }
        }
        changes
    }

    pub fn commit(&mut self) -> Result<Vec<PathBuf>, EditorError> {
        let mut changed_files: Vec<PathBuf> = vec![];
        if self.template_only {
            std::fs::remove_file(&self.path)?;
            changed_files.push(self.path.clone());
            return Err(EditorError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No control file found",
            )));
        }
        match self.primary.commit() {
            Ok(files) => {
                changed_files.extend(files.iter().map(|p| p.to_path_buf()));
            }
            Err(EditorError::GeneratedFile(
                p,
                GeneratedFile {
                    template_path: tp,
                    template_type: tt,
                },
            )) => {
                if tp.is_none() {
                    return Err(EditorError::GeneratedFile(
                        p,
                        GeneratedFile {
                            template_path: tp,
                            template_type: tt,
                        },
                    ));
                }
                let changes = self.changes();
                let changed = match update_control_template(&tp.clone().unwrap(), &p, changes, true)
                {
                    Ok(changed) => changed,
                    Err(e) => return Err(EditorError::TemplateError(tp.unwrap(), e.to_string())),
                };
                changed_files = if changed {
                    vec![tp.as_ref().unwrap().to_path_buf(), p]
                } else {
                    vec![]
                };
            }
            Err(EditorError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let template_path = if let Some(p) = find_template_path(&self.path) {
                    p
                } else {
                    return Err(EditorError::IoError(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "No control file or template found",
                    )));
                };
                let changed = match update_control_template(
                    &template_path,
                    &self.path,
                    self.changes(),
                    !self.template_only,
                ) {
                    Ok(changed) => changed,
                    Err(e) => return Err(EditorError::TemplateError(template_path, e.to_string())),
                };
                if changed {
                    changed_files.push(template_path.clone());
                    changed_files.push(self.path.clone());
                }
            }
            Err(e) => return Err(e),
        }

        Ok(changed_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_description() {
        let summary = "Summary";
        let long_description = vec!["Long", "Description"];
        let expected = "Summary\n Long\n Description\n";
        assert_eq!(format_description(summary, long_description), expected);
    }

    #[test]
    fn test_resolve_cdbs_conflicts() {
        let val = resolve_cdbs_template(
            ("Source", "libnetsds-perl"),
            "Build-Depends",
            Some("debhelper (>= 6), foo"),
            Some("@cdbs@, debhelper (>= 9)"),
            Some("debhelper (>= 10), foo"),
        )
        .unwrap();

        assert_eq!(val, Some("@cdbs@, debhelper (>= 10)".to_string()));

        let val = resolve_cdbs_template(
            ("Source", "libnetsds-perl"),
            "Build-Depends",
            Some("debhelper (>= 6), foo"),
            Some("@cdbs@, foo"),
            Some("debhelper (>= 10), foo"),
        )
        .unwrap();
        assert_eq!(val, Some("@cdbs@, foo, debhelper (>= 10)".to_string()));
        let val = resolve_cdbs_template(
            ("Source", "libnetsds-perl"),
            "Build-Depends",
            Some("debhelper (>= 6), foo"),
            Some("@cdbs@, debhelper (>= 9)"),
            Some("debhelper (>= 10), foo"),
        )
        .unwrap();
        assert_eq!(val, Some("@cdbs@, debhelper (>= 10)".to_string()));
    }

    mod guess_template_type {

        #[test]
        fn test_rules_generates_control() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/rules"),
                r#"%:
	dh $@

debian/control: debian/control.in
	cp $@ $<
"#,
            )
            .unwrap();
            assert_eq!(
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                ),
                Some(super::TemplateType::Rules)
            );
        }

        #[test]
        fn test_rules_generates_control_percent() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/rules"),
                r#"%:
	dh $@

debian/%: debian/%.in
	cp $@ $<
"#,
            )
            .unwrap();
            assert_eq!(
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                ),
                Some(super::TemplateType::Rules)
            );
        }

        #[test]
        fn test_rules_generates_control_blends() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/rules"),
                r#"%:
	dh $@

include /usr/share/blends-dev/rules
"#,
            )
            .unwrap();
            assert_eq!(
                super::guess_template_type(
                    &td.path().join("debian/control.stub"),
                    Some(&td.path().join("debian"))
                ),
                Some(super::TemplateType::Rules)
            );
        }

        #[test]
        fn test_empty_template() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            // No paragraph
            std::fs::write(td.path().join("debian/control.in"), "").unwrap();

            assert_eq!(
                None,
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_build_depends_cdbs() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Build-Depends: cdbs
Vcs-Git: file://

Package: bar
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Cdbs),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_no_build_depends() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Vcs-Git: file://

Package: bar
"#,
            )
            .unwrap();
            assert_eq!(
                None,
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_gnome() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Foo @GNOME_TEAM@
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Gnome),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_gnome_build_depends() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Build-Depends: gnome-pkg-tools, libc6-dev
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Gnome),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_cdbs() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Build-Depends: debhelper, cdbs
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Cdbs),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_multiple_paragraphs() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Build-Depends: debhelper, cdbs

Package: foo
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Cdbs),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_directory() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::create_dir(td.path().join("debian/control.in")).unwrap();
            assert_eq!(
                Some(super::TemplateType::Directory),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }

        #[test]
        fn test_debcargo() {
            let td = tempfile::tempdir().unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/control.in"),
                r#"Source: blah
Build-Depends: bar
"#,
            )
            .unwrap();
            std::fs::write(
                td.path().join("debian/debcargo.toml"),
                r#"maintainer = Joe Example <joe@example.com>
"#,
            )
            .unwrap();
            assert_eq!(
                Some(super::TemplateType::Debcargo),
                super::guess_template_type(
                    &td.path().join("debian/control.in"),
                    Some(&td.path().join("debian"))
                )
            );
        }
    }

    #[test]
    fn test_postgresql() {
        let td = tempfile::tempdir().unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/control.in"),
            r#"Source: blah
Build-Depends: bar, postgresql

Package: foo-PGVERSION
"#,
        )
        .unwrap();
        assert_eq!(
            Some(super::TemplateType::Postgresql),
            super::guess_template_type(
                &td.path().join("debian/control.in"),
                Some(&td.path().join("debian"))
            )
        );
    }

    #[test]
    fn test_apply_changes() {
        let mut deb822: deb822_lossless::Deb822 = r#"Source: blah
Build-Depends: debhelper (>= 6), foo

Package: bar
"#
        .parse()
        .unwrap();

        let mut changes = Deb822Changes(std::collections::HashMap::new());
        changes.0.insert(
            ("Source".to_string(), "blah".to_string()),
            vec![(
                "Build-Depends".to_string(),
                Some("debhelper (>= 6), foo".to_string()),
                Some("debhelper (>= 10), foo".to_string()),
            )],
        );

        super::apply_changes(&mut deb822, changes, None).unwrap();

        assert_eq!(
            deb822.to_string(),
            r#"Source: blah
Build-Depends: debhelper (>= 10), foo

Package: bar
"#
        );
    }

    #[test]
    fn test_apply_changes_new_paragraph() {
        let mut deb822: deb822_lossless::Deb822 = r#"Source: blah
Build-Depends: debhelper (>= 6), foo

Package: bar
"#
        .parse()
        .unwrap();

        let mut changes = Deb822Changes(std::collections::HashMap::new());
        changes.0.insert(
            ("Source".to_string(), "blah".to_string()),
            vec![(
                "Build-Depends".to_string(),
                Some("debhelper (>= 6), foo".to_string()),
                Some("debhelper (>= 10), foo".to_string()),
            )],
        );
        changes.0.insert(
            ("Package".to_string(), "blah2".to_string()),
            vec![
                ("Package".to_string(), None, Some("blah2".to_string())),
                (
                    "Description".to_string(),
                    None,
                    Some("Some package".to_string()),
                ),
            ],
        );

        super::apply_changes(&mut deb822, changes, None).unwrap();

        assert_eq!(
            deb822.to_string(),
            r#"Source: blah
Build-Depends: debhelper (>= 10), foo

Package: bar

Package: blah2
Description: Some package
"#
        );
    }

    #[test]
    fn test_apply_changes_conflict() {
        let mut deb822: deb822_lossless::Deb822 = r#"Source: blah
Build-Depends: debhelper (>= 6), foo

Package: bar
"#
        .parse()
        .unwrap();

        let mut changes = Deb822Changes(std::collections::HashMap::new());
        changes.0.insert(
            ("Source".to_string(), "blah".to_string()),
            vec![(
                "Build-Depends".to_string(),
                Some("debhelper (>= 7), foo".to_string()),
                Some("debhelper (>= 10), foo".to_string()),
            )],
        );

        let result = super::apply_changes(&mut deb822, changes, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err,
            ChangeConflict {
                para_key: ("Source".to_string(), "blah".to_string()),
                field: "Build-Depends".to_string(),
                actual_old_value: Some("debhelper (>= 7), foo".to_string()),
                template_old_value: Some("debhelper (>= 6), foo".to_string()),
                actual_new_value: Some("debhelper (>= 10), foo".to_string()),
            }
        );
    }

    #[test]
    fn test_apply_changes_resolve_conflict() {
        let mut deb822: deb822_lossless::Deb822 = r#"Source: blah
Build-Depends: debhelper (>= 6), foo

Package: bar
"#
        .parse()
        .unwrap();

        let mut changes = Deb822Changes(std::collections::HashMap::new());
        changes.0.insert(
            ("Source".to_string(), "blah".to_string()),
            vec![(
                "Build-Depends".to_string(),
                Some("debhelper (>= 7), foo".to_string()),
                Some("debhelper (>= 10), foo".to_string()),
            )],
        );

        let result = super::apply_changes(&mut deb822, changes, Some(|_, _, _, _, _| Ok(None)));
        assert!(result.is_ok());
        assert_eq!(
            deb822.to_string(),
            r#"Source: blah

Package: bar
"#
        );
    }
}
