use crate::{FixerError, FixerResult};
use debian_control::lossless::Control;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");
    let control_content = std::fs::read_to_string(&control_path)?;
    let control = Control::from_str(&control_content)
        .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {:?}", e)))?;

    let mut source = control
        .source()
        .ok_or_else(|| FixerError::Other("No source paragraph in debian/control".to_string()))?;

    // Get Build-Depends-Indep
    let mut build_depends_indep = match source.build_depends_indep() {
        Some(deps) => deps,
        None => return Err(FixerError::NoChanges),
    };

    // Get debhelper-compat relation from Build-Depends-Indep
    let (_pos, debhelper_compat_entry) = match build_depends_indep.get_relation("debhelper-compat")
    {
        Ok(result) => result,
        Err(_) => return Err(FixerError::NoChanges),
    };

    // Remove debhelper-compat from Build-Depends-Indep
    let removed = build_depends_indep.drop_dependency("debhelper-compat");
    if !removed {
        return Err(FixerError::NoChanges);
    }

    // Add debhelper-compat to Build-Depends (position determined by sorting)
    let mut build_depends = source.build_depends().unwrap_or_default();
    build_depends.add_dependency(debhelper_compat_entry, None);

    source.set_build_depends(&build_depends);

    // Update or remove Build-Depends-Indep
    if build_depends_indep.is_empty() {
        // Remove the field entirely using the underlying Paragraph
        source.as_mut_deb822().remove("Build-Depends-Indep");
    } else {
        source.set("Build-Depends-Indep", &build_depends_indep.to_string());
    }

    // Write back to file
    std::fs::write(&control_path, control.to_string())?;

    Ok(
        FixerResult::builder("Move debhelper-compat from Build-Depends-Indep to Build-Depends.")
            .build(),
    )
}

declare_fixer! {
    name: "debhelper-compat-wrong-field",
    tags: [],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {

    use debian_control::lossless::Control;
    use std::str::FromStr;

    #[test]
    fn test_move_debhelper_compat() {
        let input = r#"Source: blah
Build-Depends-Indep: debhelper-compat (= 12)
Build-Depends: python3-dulwich

Package: blah
Description: blah
"#;

        let control = Control::from_str(input).unwrap();
        let source = control.source().unwrap();

        // Check that debhelper-compat is in Build-Depends-Indep
        let build_depends_indep = source.build_depends_indep().unwrap();
        assert!(build_depends_indep.has_relation("debhelper-compat"));

        // The actual moving logic is tested by the integration test
    }

    #[test]
    fn test_no_change_when_not_in_build_depends_indep() {
        let input = r#"Source: blah
Build-Depends: debhelper-compat (= 12), python3-dulwich

Package: blah
Description: blah
"#;

        let control = Control::from_str(input).unwrap();
        let source = control.source().unwrap();

        // Check that debhelper-compat is NOT in Build-Depends-Indep
        assert!(source.build_depends_indep().is_none());
    }
}
