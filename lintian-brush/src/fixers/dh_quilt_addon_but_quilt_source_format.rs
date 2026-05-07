use crate::declare_detector;
use crate::diagnostic::{Action, Diagnostic, MakefileAction};
use crate::workspace::FixerWorkspace;
use crate::{FixerError, FixerPreferences, LintianIssue};
use regex::bytes::Regex;
use std::path::PathBuf;

pub fn detect(
    ws: &dyn FixerWorkspace,
    _preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    if ws.source_format()?.as_deref() != Some("3.0 (quilt)") {
        return Ok(Vec::new());
    }

    let rules_rel = PathBuf::from("debian/rules");
    let makefile = match ws.parsed_rules() {
        Ok(m) => m,
        Err(FixerError::NoChanges) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    // Skip when QUILT_PATCH_DIR points somewhere other than the default;
    // the addon's `--with quilt` may be load-bearing in that case.
    if let Some(var_def) = makefile.find_variable("QUILT_PATCH_DIR").next() {
        if let Some(patch_dir) = var_def.raw_value() {
            if patch_dir.trim() != "debian/patches" {
                return Ok(Vec::new());
            }
        }
    }

    let mut issues: Vec<LintianIssue> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    for rule in makefile.rules() {
        let Some(target) = rule.targets().next() else {
            continue;
        };
        for recipe_node in rule.recipe_nodes() {
            let recipe = recipe_node.text();
            let new_bytes = dh_invoke_drop_with(recipe.as_bytes(), b"quilt");
            if new_bytes.as_slice() == recipe.as_bytes() {
                continue;
            }
            let Ok(new_recipe) = std::str::from_utf8(&new_bytes) else {
                continue;
            };
            issues.push(LintianIssue::source_with_info(
                "dh-quilt-addon-but-quilt-source-format",
                vec!["[debian/rules]".to_string()],
            ));
            actions.push(Action::Makefile(MakefileAction::ReplaceRecipe {
                file: rules_rel.clone(),
                target: target.clone(),
                recipe: recipe.to_string(),
                new_recipe: new_recipe.to_string(),
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, issue) in issues.into_iter().enumerate() {
        let plan_actions = if i == 0 { actions.clone() } else { Vec::new() };
        diagnostics.push(Diagnostic::with_actions(
            issue,
            "dh quilt addon used with '3.0 (quilt)' source format.",
            "Don't specify --with=quilt, since package uses '3.0 (quilt)' source format.",
            plan_actions,
        ));
    }
    Ok(diagnostics)
}

fn dh_invoke_drop_with(line: &[u8], with_argument: &[u8]) -> Vec<u8> {
    if !line
        .windows(with_argument.len())
        .any(|w| w == with_argument)
    {
        return line.to_vec();
    }
    let arg_str = std::str::from_utf8(with_argument).unwrap();
    let mut result = line.to_vec();
    let re1 = Regex::new(&format!(
        r"[ \t]--with[ =]{}( .+|)$",
        regex::escape(arg_str)
    ))
    .unwrap();
    result = re1.replace_all(&result, &b"$1"[..]).to_vec();
    let re2 = Regex::new(&format!(r"([ \t])--with([ =]){},", regex::escape(arg_str))).unwrap();
    result = re2.replace_all(&result, &b"$1--with$2"[..]).to_vec();
    let re3 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}([ ,])",
        regex::escape(arg_str)
    ))
    .unwrap();
    result = re3.replace_all(&result, &b"$1--with$2$3$4"[..]).to_vec();
    let re4 = Regex::new(&format!(
        r"([ \t])--with([ =])(.+),{}$",
        regex::escape(arg_str)
    ))
    .unwrap();
    result = re4.replace_all(&result, &b"$1--with$2$3"[..]).to_vec();
    result
}

declare_detector! {
    name: "dh-quilt-addon-but-quilt-source-format",
    tags: ["dh-quilt-addon-but-quilt-source-format"],
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::DetectorAdapter;
    use crate::{FixerPreferences, Version};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn run_apply(base: &Path) -> Result<crate::FixerResult, FixerError> {
        let v: Version = "1.0".parse().unwrap();
        let adapter = DetectorAdapter::new(Box::new(DetectorImpl));
        adapter.apply(base, "test", &v, &FixerPreferences::default())
    }

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
    fn test_dh_invoke_drop_with_middle_of_list() {
        assert_eq!(
            dh_invoke_drop_with(b"\tdh $@ --with=foo,quilt,bar", b"quilt"),
            b"\tdh $@ --with=foo,bar"
        );
    }

    #[test]
    fn test_removes_with_quilt_simple() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("format"), "3.0 (quilt)\n").unwrap();
        let rules = tmp.path().join("debian/rules");
        fs::write(&rules, "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n").unwrap();

        run_apply(tmp.path()).unwrap();
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        );
    }

    #[test]
    fn test_no_change_when_not_quilt_format() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("format"), "3.0 (native)\n").unwrap();
        fs::write(
            tmp.path().join("debian/rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_custom_patch_directory() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("debian/source");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(source_dir.join("format"), "3.0 (quilt)\n").unwrap();
        fs::write(
            tmp.path().join("debian/rules"),
            "#!/usr/bin/make -f\n\nexport QUILT_PATCH_DIR = debian/patches-applied\n\n%:\n\tdh $@ --with=quilt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_no_change_when_no_format_file() {
        let tmp = TempDir::new().unwrap();
        let debian = tmp.path().join("debian");
        fs::create_dir_all(&debian).unwrap();
        fs::write(
            debian.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@ --with=quilt\n",
        )
        .unwrap();
        assert!(matches!(run_apply(tmp.path()), Err(FixerError::NoChanges)));
    }
}
