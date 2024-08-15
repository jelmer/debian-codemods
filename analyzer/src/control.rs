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
    UnknownTemplating(PathBuf, PathBuf),
}

impl std::fmt::Display for TemplateExpansionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TemplateExpansionError::Failed(s) => write!(f, "Failed: {}", s),
            TemplateExpansionError::ExpandCommandMissing(s) => {
                write!(f, "Command not found: {}", s)
            }
            TemplateExpansionError::UnknownTemplating(p1, p2) => {
                write!(
                    f,
                    "Unknown templating: {} -> {}",
                    p1.display(),
                    p2.display()
                )
            }
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
            template_path.to_path_buf(),
        )),
    }
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
