use crate::{declare_fixer, FixerError, FixerResult};
use debian_control::lossless::Control;
use std::path::Path;
use std::str::FromStr;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    let control_path = base_path.join("debian/control");

    if !control_path.exists() {
        return Err(FixerError::NoChanges);
    }

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

    // Check if libmodule-build-perl is in Build-Depends-Indep
    let (_pos, libmodule_build_entry) =
        match build_depends_indep.get_relation("libmodule-build-perl") {
            Ok(result) => result,
            Err(_) => return Err(FixerError::NoChanges),
        };

    // Remove libmodule-build-perl from Build-Depends-Indep
    let removed = build_depends_indep.drop_dependency("libmodule-build-perl");
    if !removed {
        return Err(FixerError::NoChanges);
    }

    // Add libmodule-build-perl to Build-Depends
    let mut build_depends = source.build_depends().unwrap_or_default();
    build_depends.add_dependency(libmodule_build_entry, None);
    source.set_build_depends(&build_depends);

    // Update or remove Build-Depends-Indep
    if build_depends_indep.is_empty() {
        source.as_mut_deb822().remove("Build-Depends-Indep");
    } else {
        source.set("Build-Depends-Indep", &build_depends_indep.to_string());
    }

    // Write back to file
    std::fs::write(&control_path, control.to_string())?;

    Ok(
        FixerResult::builder(
            "Move libmodule-build-perl from Build-Depends-Indep to Build-Depends.",
        )
        .fixed_tags(vec!["libmodule-build-perl-needs-to-be-in-build-depends"])
        .build(),
    )
}

declare_fixer! {
    name: "libmodule-build-perl-needs-to-be-in-build-depends",
    tags: ["libmodule-build-perl-needs-to-be-in-build-depends"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_simple() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: libtest-mock-guard-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9)\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , libmodule-build-perl\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-mock-guard-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Simple mock test library\n A mock test library.\n",
        )
        .unwrap();

        let result = run(base_path).unwrap();
        assert_eq!(
            result.description,
            "Move libmodule-build-perl from Build-Depends-Indep to Build-Depends."
        );

        let content = fs::read_to_string(&control_path).unwrap();
        assert_eq!(
            content,
            "Source: libtest-mock-guard-perl\nSection: perl\nPriority: optional\nMaintainer: Joe Maintainer <joe@example.com>\nBuild-Depends: debhelper (>= 9), libmodule-build-perl\nBuild-Depends-Indep: libclass-load-perl (>= 0.06)\n , perl\nStandards-Version: 3.9.6\n\nPackage: libtest-mock-guard-perl\nArchitecture: all\nDepends: ${misc:Depends}, ${perl:Depends}\n , libclass-load-perl (>= 0.06)\nDescription: Simple mock test library\n A mock test library.\n"
        );
    }

    #[test]
    fn test_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let control_path = debian_dir.join("control");
        fs::write(
            &control_path,
            "Source: test\nBuild-Depends: debhelper (>= 9), libmodule-build-perl\n\nPackage: test\nDescription: Test\n",
        )
        .unwrap();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_control_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        let result = run(base_path);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
