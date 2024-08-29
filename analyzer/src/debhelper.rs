use std::path::Path;

fn parse_debhelper_compat(s: &str) -> Option<u8> {
    s.split_once('#').map_or(s, |s| s.0).trim().parse().ok()
}

/// Read a debian/compat file.
///
/// # Arguments
/// * `path` - The path to the debian/compat file.
pub fn read_debhelper_compat_file(path: &Path) -> Result<Option<u8>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse_debhelper_compat(&content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn get_debhelper_compat_level_from_control(control: &debian_control::Control) -> Option<u8> {
    let source = control.source()?;

    let build_depends = source.build_depends()?;

    let rels = build_depends
        .entries()
        .flat_map(|entry| entry.relations().collect::<Vec<_>>())
        .find(|r| r.name() == "debhelper-compat");

    rels.and_then(|r| r.version().and_then(|v| v.1.to_string().parse().ok()))
}

pub fn get_debhelper_compat_level(path: &Path) -> Result<Option<u8>, std::io::Error> {
    match read_debhelper_compat_file(&path.join("debian/compat")) {
        Ok(Some(level)) => {
            return Ok(Some(level));
        }
        Err(e) => {
            return Err(e);
        }
        Ok(None) => {}
    }

    let p = path.join("debian/control");

    match std::fs::File::open(&p) {
        Ok(f) => {
            let control = debian_control::Control::read_relaxed(f).unwrap().0;
            Ok(get_debhelper_compat_level_from_control(&control))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_debhelper_compat() {
        assert_eq!(super::parse_debhelper_compat("9"), Some(9));
        assert_eq!(super::parse_debhelper_compat("9 # comment"), Some(9));
        assert_eq!(
            super::parse_debhelper_compat("9 # comment # comment"),
            Some(9)
        );
        assert_eq!(super::parse_debhelper_compat(""), None);
        assert_eq!(super::parse_debhelper_compat(" # comment"), None);
    }

    #[test]
    fn test_get_debhelper_compat_level_from_control() {
        let text = "Source: foo
Build-Depends: debhelper-compat (= 9)

Package: foo
Architecture: any
";

        let control = debian_control::Control::read_relaxed(&mut text.as_bytes())
            .unwrap()
            .0;

        assert_eq!(
            super::get_debhelper_compat_level_from_control(&control),
            Some(9)
        );
    }
}
