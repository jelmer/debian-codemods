use crate::{declare_fixer, FixerError, FixerResult};
use makefile_lossless::Makefile;
use regex::bytes::Regex;
use std::fs;
use std::path::Path;

pub fn run(base_path: &Path) -> Result<FixerResult, FixerError> {
    // Check if debian/source/format exists and is "3.0 (quilt)"
    let source_format_path = base_path.join("debian/source/format");
    if !source_format_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let format = fs::read_to_string(&source_format_path)?.trim().to_string();
    if format != "3.0 (quilt)" {
        return Err(FixerError::NoChanges);
    }

    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Err(FixerError::NoChanges);
    }

    let content = fs::read_to_string(&rules_path)?;
    let makefile: Makefile = Makefile::read_relaxed(content.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse makefile: {}", e)))?;

    // Check if QUILT_PATCH_DIR is set to something other than "debian/patches"
    if let Some(var_def) = makefile.find_variable("QUILT_PATCH_DIR").next() {
        if let Some(patch_dir) = var_def.raw_value() {
            let patch_dir_str = patch_dir.trim();
            if patch_dir_str != "debian/patches" {
                // Custom patch directory, don't modify
                return Err(FixerError::NoChanges);
            }
        }
    }

    // Process the makefile to remove --with quilt from dh commands
    let mut made_changes = false;

    for mut rule in makefile.rules() {
        for (recipe_index, recipe) in rule.recipes().enumerate() {
            let new_recipe = dh_invoke_drop_with(recipe.as_bytes(), b"quilt");
            if new_recipe.as_slice() != recipe.as_bytes() {
                // Recipe changed, replace it
                if rule.replace_command(
                    recipe_index,
                    std::str::from_utf8(&new_recipe).unwrap_or(&recipe),
                ) {
                    made_changes = true;
                }
            }
        }
    }

    if !made_changes {
        return Err(FixerError::NoChanges);
    }

    // Write back the modified makefile
    fs::write(&rules_path, makefile.to_string())?;

    Ok(FixerResult::builder(
        "Don't specify --with=quilt, since package uses '3.0 (quilt)' source format.",
    )
    .fixed_tags(vec!["dh-quilt-addon-but-quilt-source-format"])
    .build())
}

/// Drop a particular value from a --with argument in a dh command line.
/// This is a port of debmutate._rules.dh_invoke_drop_with from Python.
fn dh_invoke_drop_with(line: &[u8], with_argument: &[u8]) -> Vec<u8> {
    // Check if the with_argument is even in the line
    if !line
        .windows(with_argument.len())
        .any(|w| w == with_argument)
    {
        return line.to_vec();
    }

    let mut result = line.to_vec();

    // It's the only with argument: --with quilt or --with=quilt at end
    let re1 = Regex::new(&format!(
        r"[ \t]--with[ =]{}( .+|)$",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re1.replace_all(&result, &b"$1"[..]).to_vec();

    // It's at the beginning of a comma-separated list: --with=quilt,foo
    let re2 = Regex::new(&format!(
        r"([ \t])--with([ =]){},",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re2.replace_all(&result, &b"$1--with$2"[..]).to_vec();

    // It's somewhere in the middle or the end: --with=foo,quilt,bar or --with=foo,quilt
    let re3 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}([ ,])",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re3.replace_all(&result, &b"$1--with$2$3$4"[..]).to_vec();

    // It's at the end: --with=foo,quilt$
    let re4 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}$",
        regex::escape(std::str::from_utf8(with_argument).unwrap())
    ))
    .unwrap();
    result = re4.replace_all(&result, &b"$1--with$2$3"[..]).to_vec();

    result
}

declare_fixer! {
    name: "dh-quilt-addon-but-quilt-source-format",
    tags: ["dh-quilt-addon-but-quilt-source-format"],
    apply: |basedir, _package, _version, _preferences| {
        run(basedir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_fixers::BuiltinFixer;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_dh_invoke_drop_with_only_argument() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=quilt", b"quilt"),
            b"\tdh $@"
        );
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with quilt", b"quilt"),
            b"\tdh $@"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_first_in_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=quilt,autoreconf", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_last_in_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=autoreconf,quilt", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_middle_of_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=foo,quilt,bar", b"quilt"),
            b"\tdh $@ --with=foo,bar"
        );
    }

    #[test]
    fn test_dh_invoke_drop_with_not_present() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=autoreconf", b"quilt"),
            b"\tdh $@ --with=autoreconf"
        );
    }

    #[test]
    fn test_removes_with_quilt_simple() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_content = "3.0 (quilt)\n";
        fs::write(source_dir.join("format"), format_content).unwrap();

        let rules_content = "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(&rules_path).unwrap();
        assert!(!updated_content.contains("--with=quilt"));
        assert!(!updated_content.contains("--with quilt"));
        assert!(updated_content.contains("dh $@"));
    }

    #[test]
    fn test_no_change_when_not_quilt_format() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_content = "3.0 (native)\n";
        fs::write(source_dir.join("format"), format_content).unwrap();

        let rules_content = "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_custom_patch_directory() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        let source_dir = debian_dir.join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let format_content = "3.0 (quilt)\n";
        fs::write(source_dir.join("format"), format_content).unwrap();

        let rules_content =
            "#!/usr/bin/make -f\n\nexport QUILT_PATCH_DIR = debian/pathces-applies\n\n%:\n\tdh $@ --with=quilt\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_format_file() {
        let temp_dir = TempDir::new().unwrap();
        let debian_dir = temp_dir.path().join("debian");
        fs::create_dir_all(&debian_dir).unwrap();

        let rules_content = "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n";
        let rules_path = debian_dir.join("rules");
        fs::write(&rules_path, rules_content).unwrap();

        let fixer = FixerImpl;
        let version: crate::Version = "1.0".parse().unwrap();
        let result = fixer.apply(
            temp_dir.path(),
            "test-package",
            &version,
            &Default::default(),
        );
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
