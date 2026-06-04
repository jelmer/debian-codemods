use crate::debhelper::detect_debhelper_buildsystem;
use crate::declare_detector;
use crate::diagnostic::{
    Action, Deb822Action, Diagnostic, FilesystemAction, LintianOverridesAction,
    OverrideLineSelector, ParagraphSelector,
};
use crate::{FixerError, FixerPreferences, LintianIssue, Visibility};
use debian_analyzer::debhelper::{
    lowest_non_deprecated_compat_level, maximum_debhelper_compat_version,
    read_debhelper_compat_file,
};
use debian_control::lossless::relations::Relations;
use debian_workspace::Workspace;
use debversion::Version;
use makefile_lossless::{Makefile, Rule};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn autoreconf_disabled(ws: &dyn Workspace) -> bool {
    let Ok(mf) = ws.parsed_rules() else {
        return false;
    };

    // Check for --without.*autoreconf in any recipe
    for rule in mf.rules() {
        for recipe in rule.recipes() {
            if recipe.contains("--without") && recipe.contains("autoreconf") {
                return true;
            }
        }
    }

    // Check if override_dh_autoreconf exists and is empty
    for rule in mf.rules_by_target("override_dh_autoreconf") {
        if rule.recipe_count() == 0 {
            return true;
        }
    }

    false
}

fn get_current_package_version(ws: &dyn Workspace) -> Result<Version, FixerError> {
    let changelog = match ws.parsed_changelog() {
        Ok(c) => c,
        // If no changelog exists, return default version
        Err(debian_workspace::Error::NotFound) => return Ok("1.0-1".parse().unwrap()),
        Err(e) => return Err(e.into()),
    };

    let entries: Vec<_> = changelog.iter().collect();
    if let Some(entry) = entries.first() {
        entry
            .version()
            .ok_or_else(|| FixerError::Other("No version in changelog entry".to_string()))
    } else {
        Err(FixerError::Other("No entries in changelog".to_string()))
    }
}

// Transformation tracking
struct Transformations {
    subitems: HashSet<String>,
    /// Lintian tags whose overrides become unused once the matching
    /// construct is rewritten out of debian/rules. Populated alongside the
    /// transform that removes the construct, so an override is only dropped
    /// when the change that obsoletes it was actually made. See bug #970174.
    stale_override_tags: HashSet<String>,
}

impl Transformations {
    fn new() -> Self {
        Self {
            subitems: HashSet::new(),
            stale_override_tags: HashSet::new(),
        }
    }

    fn add(&mut self, item: impl Into<String>) {
        self.subitems.insert(item.into());
    }

    fn remove(&mut self, item: &str) {
        self.subitems.remove(item);
    }

    /// Record that overrides for `tag` are now stale.
    fn add_stale_tag(&mut self, tag: impl Into<String>) {
        self.stale_override_tags.insert(tag.into());
    }
}

/// Read binary package names from debian/control via the workspace.
fn binary_package_names(ws: &dyn Workspace) -> Result<Vec<String>, FixerError> {
    let Ok(control) = ws.parsed_control() else {
        return Ok(Vec::new());
    };
    Ok(control.binaries().filter_map(|b| b.name()).collect())
}

// Upgrade to debhelper 10
fn upgrade_to_debhelper_10(
    ws: &dyn Workspace,
    actions: &mut Vec<Action>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    // dh_installinit will no longer install a file named debian/package
    // as an init script.
    for name in binary_package_names(ws)? {
        let old_rel = PathBuf::from("debian").join(&name);
        if ws.read_file(&old_rel)?.is_none() {
            continue;
        }
        let new_rel = PathBuf::from("debian").join(format!("{}.init", name));
        actions.push(Action::Filesystem(FilesystemAction::Rename {
            file: old_rel,
            to: new_rel,
        }));
        transforms.add(format!("Rename debian/{} to debian/{}.init.", name, name));
    }
    Ok(())
}

// Upgrade to debhelper 11
fn upgrade_to_debhelper_11(
    ws: &dyn Workspace,
    actions: &mut Vec<Action>,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    upgrade_to_installsystemd(ws, rules_mf, transforms)?;

    // Drop debian/*.upstart files and add rm_conffile to maintscript
    let Ok(Some(entries)) = ws.list_dir(Path::new("debian")) else {
        return Ok(());
    };

    for name_str in entries {
        let parts: Vec<&str> = name_str.split('.').collect();
        if parts.last() != Some(&"upstart") {
            continue;
        }

        let (package, service) = if parts.len() == 3 {
            (parts[0], parts[1])
        } else if parts.len() == 2 {
            (parts[0], parts[0])
        } else {
            continue;
        };

        let file_rel = PathBuf::from("debian").join(&name_str);
        actions.push(Action::Filesystem(FilesystemAction::Delete {
            file: file_rel,
        }));
        transforms.add(format!("Drop obsolete upstart file {}.", name_str));

        // Add maintscript entry: append `rm_conffile` to the package's
        // maintscript (creating the file if it doesn't exist).
        let current_version = get_current_package_version(ws)?;
        let maintscript_rel = PathBuf::from("debian").join(format!("{}.maintscript", package));
        let existing = ws
            .read_file(&maintscript_rel)?
            .and_then(|b| String::from_utf8(b.into_owned()).ok())
            .unwrap_or_default();

        let rm_conffile_line = format!(
            "rm_conffile /etc/init/{}.conf {}\n",
            service, current_version
        );

        if !existing.contains(&rm_conffile_line) {
            let mut new_content = existing;
            new_content.push_str(&rm_conffile_line);
            actions.push(Action::Filesystem(FilesystemAction::Write {
                file: maintscript_rel,
                content: new_content.into_bytes(),
            }));
        }
    }

    Ok(())
}

/// Lazily open debian/rules for in-memory mutation. Returns `None` if
/// the file doesn't exist.
fn get_rules<'a>(
    ws: &dyn Workspace,
    rules_mf: &'a mut Option<Makefile>,
) -> Result<Option<&'a mut Makefile>, FixerError> {
    if rules_mf.is_none() {
        match ws.parsed_rules() {
            Ok(mf) => *rules_mf = Some(mf),
            Err(debian_workspace::Error::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(rules_mf.as_mut())
}

fn upgrade_to_installsystemd(
    ws: &dyn Workspace,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let Some(mf) = get_rules(ws, rules_mf)? else {
        return Ok(());
    };

    for mut rule in mf.rules() {
        let targets: Vec<String> = rule.targets().collect();

        if targets.contains(&"override_dh_systemd_enable".to_string()) {
            rule.rename_target("override_dh_systemd_enable", "override_dh_installsystemd")
                .map_err(|e| FixerError::Other(format!("Failed to rename target: {:?}", e)))?;
            transforms.add_stale_tag("debian-rules-uses-deprecated-systemd-override");
        }
        if targets.contains(&"override_dh_systemd_start".to_string()) {
            rule.rename_target("override_dh_systemd_start", "override_dh_installsystemd")
                .map_err(|e| FixerError::Other(format!("Failed to rename target: {:?}", e)))?;
            transforms.add_stale_tag("debian-rules-uses-deprecated-systemd-override");
        }

        let recipes: Vec<String> = rule.recipes().collect();
        for (recipe_idx, recipe) in recipes.iter().enumerate() {
            let mut new_recipe = recipe.clone();
            let mut recipe_changed = false;

            if new_recipe.trim_start().starts_with("dh ") {
                let old = new_recipe.clone();
                new_recipe = debian_analyzer::rules::dh_invoke_drop_with(&new_recipe, "systemd");
                if new_recipe != old {
                    transforms.add("Drop --with=systemd, no longer required.".to_string());
                    recipe_changed = true;
                }
            }
            if new_recipe.contains("dh_systemd_enable") {
                new_recipe = new_recipe.replace("dh_systemd_enable", "dh_installsystemd");
                transforms.add(
                    "Use dh_installsystemd rather than deprecated dh_systemd_enable.".to_string(),
                );
                recipe_changed = true;
            }
            if new_recipe.contains("dh_systemd_start") {
                new_recipe = new_recipe.replace("dh_systemd_start", "dh_installsystemd");
                transforms.add(
                    "Use dh_installsystemd rather than deprecated dh_systemd_start.".to_string(),
                );
                recipe_changed = true;
            }

            if recipe_changed {
                rule.replace_command(recipe_idx, &new_recipe);
            }
        }
    }

    Ok(())
}

// Upgrade to debhelper 12
fn uses_libexecdir(ws: &dyn Workspace) -> bool {
    for name in &["configure.ac", "configure.in", "Makefile.am", "meson.build"] {
        if let Ok(Some(bytes)) = ws.read_file(Path::new(name)) {
            if let Ok(content) = std::str::from_utf8(&bytes) {
                if content.contains("libexecdir") {
                    return true;
                }
            }
        }
    }
    false
}

fn upgrade_to_debhelper_12(
    ws: &dyn Workspace,
    base_path: &Path,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    update_rules_for_compat_12(ws, base_path, rules_mf, transforms)?;

    // In compat 12, autoconf and meson no longer pass --libexecdir explicitly.
    // Add an override so files still end up in /usr/libexec.
    // detect_debhelper_buildsystem invokes `dh_assistant` with a working
    // directory, which only the on-disk host can supply.
    let buildsystem = match detect_debhelper_buildsystem(base_path, None) {
        Ok(Some(bs)) if bs == "autoconf" || bs == "meson" => bs,
        _ => return Ok(()),
    };
    if !uses_libexecdir(ws) {
        return Ok(());
    }

    let Some(mf) = get_rules(ws, rules_mf)? else {
        return Ok(());
    };

    let mut changed = false;
    let has_override = mf
        .rules()
        .any(|rule| rule.targets().any(|t| t == "override_dh_auto_configure"));

    if has_override {
        for mut rule in mf.rules() {
            if !rule.targets().any(|t| t == "override_dh_auto_configure") {
                continue;
            }
            let recipes: Vec<String> = rule.recipes().collect();
            for (idx, recipe) in recipes.iter().enumerate() {
                if !recipe.trim_start().starts_with("dh_auto_configure") {
                    continue;
                }
                if let Some(sep_pos) = recipe.find(" -- ") {
                    let (before, after) = recipe.split_at(sep_pos + 4);
                    let new_after = crate::rules::dh_invoke_set_option_argument_soft(
                        after,
                        "--libexecdir",
                        "/usr/libexec",
                    );
                    if new_after != after {
                        let new_recipe = format!("{}{}", before, new_after);
                        rule.replace_command(idx, &new_recipe);
                        changed = true;
                    }
                } else {
                    let new_recipe = format!("{} -- --libexecdir=/usr/libexec", recipe.trim_end());
                    rule.replace_command(idx, &new_recipe);
                    changed = true;
                }
            }
        }
    } else {
        let new_rule = Rule::new(
            &["override_dh_auto_configure"],
            &[],
            &["dh_auto_configure -- --libexecdir=/usr/libexec"],
        );
        let num_rules = mf.rules().count();
        mf.insert_rule(num_rules, new_rule).map_err(|e| {
            FixerError::Other(format!(
                "Failed to insert override_dh_auto_configure rule: {:?}",
                e,
            ))
        })?;
        changed = true;
    }

    if changed {
        transforms.add(format!(
            "debian/rules: Add --libexecdir=/usr/libexec for {} build system.",
            buildsystem
        ));
    }

    Ok(())
}

fn update_rules_for_compat_12(
    ws: &dyn Workspace,
    base_path: &Path,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let Some(mf) = get_rules(ws, rules_mf)? else {
        return Ok(());
    };

    let mut changed = false;
    let mut need_override_missing = false;
    let mut pybuild_upgraded = false;

    let mut rules_to_remove = Vec::new();

    // Process each rule directly - mutation methods modify in place
    for (rule_idx, mut rule) in mf.rules().enumerate() {
        let targets: Vec<String> = rule.targets().collect();
        let recipes: Vec<String> = rule.recipes().collect();

        // Transform each recipe
        for (recipe_idx, recipe) in recipes.iter().enumerate() {
            let mut new_recipe = recipe.clone();
            let original_recipe = new_recipe.clone();

            // Fix dh argument order
            if new_recipe.trim_start().starts_with("dh ") {
                new_recipe = fix_dh_argument_order(&new_recipe);

                // Check if we need to add --buildsystem=pybuild
                if !new_recipe.contains("buildsystem") {
                    match detect_debhelper_buildsystem(base_path, None) {
                        Ok(Some(buildsystem)) if buildsystem == "python_distutils" => {
                            tracing::debug!(
                                "Detected python_distutils buildsystem, upgrading to pybuild"
                            );
                            new_recipe =
                                new_recipe.trim_end().to_string() + " --buildsystem=pybuild";
                            transforms.add(
                                "Replace python_distutils buildsystem with pybuild.".to_string(),
                            );
                            pybuild_upgraded = true;
                        }
                        Ok(Some(buildsystem)) => {
                            tracing::debug!(
                                "Detected buildsystem: {}, not upgrading to pybuild",
                                buildsystem
                            );
                        }
                        Ok(None) => {
                            tracing::debug!("No buildsystem detected");
                        }
                        Err(e) => {
                            tracing::debug!("Failed to detect buildsystem: {}", e);
                        }
                    }
                } else if new_recipe.contains("buildsystem=pybuild")
                    || new_recipe.contains("buildsystem pybuild")
                {
                    pybuild_upgraded = true;
                }
            }

            // Replace deprecated -s with -a
            if new_recipe.trim_start().starts_with("dh") {
                let old = new_recipe.clone();
                new_recipe =
                    debian_analyzer::rules::dh_invoke_replace_argument(&new_recipe, "-s", "-a");
                if new_recipe != old {
                    transforms.add("Replace deprecated -s with -a.".to_string());
                }
                let old = new_recipe.clone();
                new_recipe = debian_analyzer::rules::dh_invoke_replace_argument(
                    &new_recipe,
                    "--same-arch",
                    "--arch",
                );
                if new_recipe != old {
                    transforms.add("Replace deprecated --same-arch with --arch.".to_string());
                }
            }

            // Replace python_distutils buildsystem with pybuild
            if new_recipe.contains("--buildsystem=python_distutils")
                || new_recipe.contains("--buildsystem python_distutils")
                || new_recipe.contains("-O--buildsystem=python_distutils")
            {
                new_recipe =
                    new_recipe.replace("--buildsystem=python_distutils", "--buildsystem=pybuild");
                new_recipe =
                    new_recipe.replace("--buildsystem python_distutils", "--buildsystem=pybuild");
                new_recipe = new_recipe.replace(
                    "-O--buildsystem=python_distutils",
                    "-O--buildsystem=pybuild",
                );
                transforms.add("Replace python_distutils buildsystem with pybuild.".to_string());
                pybuild_upgraded = true;
            }

            // Handle PYBUILD transformation
            if (pybuild_upgraded
                || new_recipe.contains("buildsystem=pybuild")
                || new_recipe.contains("buildsystem pybuild"))
                && new_recipe.trim_start().starts_with("dh_auto_")
                && new_recipe.contains(" -- ")
            {
                if let Some((before, after)) = new_recipe.split_once(" -- ") {
                    let dh_cmd = before.split_whitespace().next().unwrap_or("");
                    if let Some(step) = dh_cmd.strip_prefix("dh_auto_") {
                        let step_upper = step.to_uppercase();
                        let args = after.trim();
                        let indent = &recipe[..recipe.len() - recipe.trim_start().len()];
                        new_recipe = format!(
                            "{}PYBUILD_{}_ARGS={} {}",
                            indent,
                            step_upper,
                            args,
                            before.trim()
                        );
                        transforms
                            .add("Replace python_distutils buildsystem with pybuild.".to_string());
                    }
                }
            }

            // Replace dh_clean -k with dh_prep
            if new_recipe.contains("dh_clean -k") {
                new_recipe = new_recipe.replace("dh_clean -k", "dh_prep");
                transforms.add("debian/rules: Replace dh_clean -k with dh_prep.".to_string());
                transforms.add_stale_tag("dh-clean-k-is-deprecated");
            }

            // Replace --no-restart-on-upgrade with --no-stop-on-upgrade
            if (new_recipe.trim_start().starts_with("dh ") || new_recipe.contains("dh_installinit"))
                && new_recipe.contains("--no-restart-on-upgrade")
            {
                new_recipe = new_recipe.replace("--no-restart-on-upgrade", "--no-stop-on-upgrade");
                transforms
                    .add("Replace --no-restart-on-upgrade with --no-stop-on-upgrade.".to_string());
            }

            // Handle --list-missing
            if new_recipe.contains("--list-missing")
                && (new_recipe.trim_start().starts_with("dh ")
                    || new_recipe.trim_start().starts_with("dh_install "))
            {
                let old = new_recipe.clone();
                new_recipe =
                    debian_analyzer::rules::dh_invoke_drop_argument(&new_recipe, "--list-missing");
                new_recipe = debian_analyzer::rules::dh_invoke_drop_argument(
                    &new_recipe,
                    "-O--list-missing",
                );
                if new_recipe != old {
                    transforms.add("debian/rules: Rely on default use of dh_missing rather than using dh_install --list-missing.".to_string());
                }
            }

            // Handle --fail-missing
            if new_recipe.contains("--fail-missing")
                && (new_recipe.trim_start().starts_with("dh ")
                    || new_recipe.trim_start().starts_with("dh_install "))
            {
                let old = new_recipe.clone();
                new_recipe =
                    debian_analyzer::rules::dh_invoke_drop_argument(&new_recipe, "--fail-missing");
                new_recipe = debian_analyzer::rules::dh_invoke_drop_argument(
                    &new_recipe,
                    "-O--fail-missing",
                );
                if new_recipe != old {
                    need_override_missing = true;
                    transforms.add(
                        "debian/rules: Move --fail-missing argument to dh_missing.".to_string(),
                    );
                }
            }

            // Replace command if it changed
            if new_recipe != original_recipe {
                rule.replace_command(recipe_idx, &new_recipe);
                changed = true;
            }
        }

        // Check if this is now an empty override_dh_install (after transformations)
        let final_recipes: Vec<String> = rule.recipes().collect();
        if targets.contains(&"override_dh_install".to_string())
            && final_recipes.len() == 1
            && final_recipes[0].trim() == "dh_install"
        {
            rules_to_remove.push(rule_idx);
        }
    }

    // Remove empty override_dh_install rules
    // We need to collect the rules and remove them by calling Rule::remove() which also removes comments
    if !rules_to_remove.is_empty() {
        let all_rules: Vec<_> = mf.rules().enumerate().collect();
        for (idx, rule) in all_rules.into_iter().rev() {
            if rules_to_remove.contains(&idx) {
                rule.remove()
                    .map_err(|e| FixerError::Other(format!("Failed to remove rule: {:?}", e)))?;
                changed = true;
            }
        }
    }

    // Add override_dh_missing if needed
    if need_override_missing {
        let has_override = mf
            .rules()
            .any(|rule| rule.targets().any(|t| t == "override_dh_missing"));
        if !has_override {
            let new_rule = Rule::new(
                &["override_dh_missing"],
                &[],
                &["dh_missing --fail-missing"],
            );
            let num_rules = mf.rules().count();
            mf.insert_rule(num_rules, new_rule).map_err(|e| {
                FixerError::Other(format!(
                    "Failed to insert override_dh_missing rule: {:?}",
                    e
                ))
            })?;
            let _ = changed; // changed flag kept for symmetry; unused now
        }
    }

    Ok(())
}

fn fix_dh_argument_order(line: &str) -> String {
    if !line.trim_start().starts_with("dh ") {
        return line.to_string();
    }

    // Preserve leading whitespace
    let indent = &line[..line.len() - line.trim_start().len()];
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return line.to_string();
    }

    // Find position of $@ or $* or ${@}
    let mut va_pos = None;
    for (i, part) in parts.iter().enumerate() {
        if *part == "$@" || *part == "$*" || *part == "${@}" {
            va_pos = Some(i);
            break;
        }
    }

    if let Some(pos) = va_pos {
        if pos > 1 {
            // Move it to position 1 (right after 'dh')
            let mut new_parts = parts.clone();
            let va = new_parts.remove(pos);
            new_parts.insert(1, va);
            return format!("{}{}", indent, new_parts.join(" "));
        }
    }

    line.to_string()
}

// Upgrade to debhelper 13
fn upgrade_to_debhelper_13(
    ws: &dyn Workspace,
    actions: &mut Vec<Action>,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    // Rename debian/*.tmpfile to debian/*.tmpfiles
    for name in binary_package_names(ws)? {
        let tmpfile_rel = PathBuf::from("debian").join(format!("{}.tmpfile", name));
        if ws.read_file(&tmpfile_rel)?.is_none() {
            continue;
        }
        let tmpfiles_rel = PathBuf::from("debian").join(format!("{}.tmpfiles", name));
        actions.push(Action::Filesystem(FilesystemAction::Rename {
            file: tmpfile_rel,
            to: tmpfiles_rel,
        }));
        transforms.add(format!(
            "Rename debian/{}.tmpfile to debian/{}.tmpfiles.",
            name, name
        ));
    }

    // Also check for generic tmpfile
    let tmpfile_rel = PathBuf::from("debian/tmpfile");
    if ws.read_file(&tmpfile_rel)?.is_some() {
        actions.push(Action::Filesystem(FilesystemAction::Rename {
            file: tmpfile_rel,
            to: PathBuf::from("debian/tmpfiles"),
        }));
        transforms.add("Rename debian/tmpfile to debian/tmpfiles.".to_string());
    }

    // Drop --fail-missing from dh_missing calls
    drop_dh_missing_fail(ws, rules_mf, transforms)?;

    // Remove DEB_BUILD_OPTIONS nocheck wrapper from override_dh_auto_test
    remove_nocheck_wrapper(ws, rules_mf, transforms)?;

    Ok(())
}

fn drop_dh_missing_fail(
    ws: &dyn Workspace,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let Some(mf) = get_rules(ws, rules_mf)? else {
        return Ok(());
    };

    let mut rules_to_remove = Vec::new();

    // Process rules directly with mutation methods
    for (rule_idx, mut rule) in mf.rules().enumerate() {
        let targets: Vec<String> = rule.targets().collect();
        let recipes: Vec<String> = rule.recipes().collect();

        for (recipe_idx, recipe) in recipes.iter().enumerate() {
            let trimmed = recipe.trim();
            if trimmed.starts_with("dh_missing ") && trimmed.contains("--fail-missing") {
                let mut new_recipe = recipe.clone();
                new_recipe = new_recipe.replace("--fail-missing", "");
                new_recipe = new_recipe.replace("-O--fail-missing", "");
                // Clean up extra spaces
                let parts: Vec<&str> = new_recipe.split_whitespace().collect();
                let indent = &recipe[..recipe.len() - recipe.trim_start().len()];
                new_recipe = format!("{}{}", indent, parts.join(" "));

                // Check if we previously added this
                if transforms
                    .subitems
                    .contains("debian/rules: Move --fail-missing argument to dh_missing.")
                {
                    transforms.remove("debian/rules: Move --fail-missing argument to dh_missing.");
                    transforms.add(
                        "debian/rules: Drop --fail-missing argument, now the default.".to_string(),
                    );
                } else {
                    transforms.add(
                        "debian/rules: Drop --fail-missing argument to dh_missing, which is now the default.".to_string(),
                    );
                }

                rule.replace_command(recipe_idx, &new_recipe);
            }
        }

        // Check if this is now an empty override_dh_missing (only contains "dh_missing" with no arguments)
        let final_recipes: Vec<String> = rule.recipes().collect();
        if targets.contains(&"override_dh_missing".to_string())
            && final_recipes.len() == 1
            && final_recipes[0].trim() == "dh_missing"
        {
            rules_to_remove.push(rule_idx);
        }
    }

    // Remove empty override_dh_missing rules
    if !rules_to_remove.is_empty() {
        let all_rules: Vec<_> = mf.rules().enumerate().collect();
        for (idx, rule) in all_rules.into_iter().rev() {
            if rules_to_remove.contains(&idx) {
                rule.remove()
                    .map_err(|e| FixerError::Other(format!("Failed to remove rule: {:?}", e)))?;
            }
        }
    }

    Ok(())
}

fn remove_nocheck_wrapper(
    ws: &dyn Workspace,
    rules_mf: &mut Option<Makefile>,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let Some(mf) = get_rules(ws, rules_mf)? else {
        return Ok(());
    };

    for rule in mf.rules() {
        let targets: Vec<String> = rule.targets().collect();
        if !targets.contains(&"override_dh_auto_test".to_string()) {
            continue;
        }

        // Iterate through rule items to find conditionals
        for item in rule.items() {
            if let makefile_lossless::RuleItem::Conditional(mut cond) = item {
                let cond_str = cond.to_string();
                if cond_str.contains("ifeq (,$(filter nocheck,$(DEB_BUILD_OPTIONS)))") {
                    transforms.add(
                        "Drop check for DEB_BUILD_OPTIONS containing \"nocheck\", since debhelper now does this.".to_string(),
                    );
                    cond.unwrap().map_err(|e| {
                        FixerError::Other(format!("Failed to unwrap conditional: {:?}", e))
                    })?;
                }
            }
        }
    }

    Ok(())
}

/// Drop override lines for tags whose construct the compat bump rewrote out of
/// debian/rules. Without this, the override is left behind as an unused
/// override after the change (bug #970174).
fn drop_stale_overrides(
    ws: &dyn Workspace,
    transforms: &mut Transformations,
    actions: &mut Vec<Action>,
) -> Result<(), FixerError> {
    if transforms.stale_override_tags.is_empty() {
        return Ok(());
    }

    for file in crate::lintian_overrides::override_files(ws)? {
        let Some(bytes) = ws.read_file(&file)? else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes.into_owned()) else {
            continue;
        };
        let Ok(parsed) = crate::lintian_overrides::LintianOverrides::parse(&text).ok() else {
            continue;
        };
        for line in parsed.lines() {
            let Some(tag) = line.tag() else {
                continue;
            };
            let tag = tag.text().to_string();
            if !transforms.stale_override_tags.contains(&tag) {
                continue;
            }
            actions.push(Action::LintianOverrides(LintianOverridesAction::DropLine {
                file: file.clone(),
                selector: OverrideLineSelector {
                    tag: tag.clone(),
                    info: line.info().map(|i| i.trim().to_string()),
                    package: line.package(),
                },
            }));
            transforms.add(format!(
                "{}: Drop now-unused override for {}.",
                file.display(),
                tag
            ));
        }
    }

    Ok(())
}

pub fn detect(
    ws: &dyn Workspace,
    preferences: &FixerPreferences,
) -> Result<Vec<Diagnostic>, FixerError> {
    // detect_debhelper_buildsystem invokes `dh_assistant` with a
    // working-directory cwd, which only the on-disk host can supply.
    // LSP hosts won't have a base path and skip.
    let Some(base_path) = ws.base_path() else {
        return Ok(Vec::new());
    };
    let base_path = base_path.to_path_buf();
    let base_path = base_path.as_path();

    let compat_release = preferences.compat_release.as_deref().unwrap_or("sid");
    let mut new_debhelper_compat_version = maximum_debhelper_compat_version(compat_release);

    // CDBS doesn't support debhelper 11 or 12 yet, cap to 10.
    if let Ok(Some(rules_bytes)) = ws.read_file(Path::new("debian/rules")) {
        if rules_bytes
            .windows(b"/usr/share/cdbs/".len())
            .any(|w| w == b"/usr/share/cdbs/")
        {
            new_debhelper_compat_version = new_debhelper_compat_version.min(10);
        }
    }

    // Autoreconf disabled and old configure → cap to 10.
    if autoreconf_disabled(ws) {
        if let Ok(Some(bytes)) = ws.read_file(Path::new("configure")) {
            if let Ok(contents) = std::str::from_utf8(&bytes) {
                if !contents.contains("runstatedir") {
                    new_debhelper_compat_version = new_debhelper_compat_version.min(10);
                }
            }
        }
    }

    let compat_rel = PathBuf::from("debian/compat");
    let control_rel = PathBuf::from("debian/control");
    let compat_bytes = ws.read_file(&compat_rel)?;
    let has_control = ws.read_file(&control_rel)?.is_some();

    let current_debhelper_compat_version: u8;
    let mut transforms = Transformations::new();
    let mut actions: Vec<Action> = Vec::new();
    // Lazy in-memory Makefile for debian/rules. If still `None` at the
    // end, no rules-touching helper opened the file.
    let mut rules_mf: Option<Makefile> = None;
    let original_rules_text = ws
        .read_file(Path::new("debian/rules"))?
        .and_then(|b| String::from_utf8(b.into_owned()).ok())
        .unwrap_or_default();

    if compat_bytes.is_some() {
        // Compat version is stored in debian/compat.
        let compat_abs = base_path.join(&compat_rel);
        current_debhelper_compat_version = match read_debhelper_compat_file(&compat_abs)? {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };
        if current_debhelper_compat_version >= new_debhelper_compat_version {
            return Ok(Vec::new());
        }

        // Update debian/compat (overwrite with new version).
        actions.push(Action::Filesystem(FilesystemAction::Write {
            file: compat_rel,
            content: format!("{}\n", new_debhelper_compat_version).into_bytes(),
        }));

        // Update Build-Depends to require the new debhelper.
        let control = ws.parsed_control()?;
        if let Some(source) = control.source() {
            let mut build_depends = source.build_depends().unwrap_or_default();
            let version = Version::from_str(&format!("{}~", new_debhelper_compat_version))
                .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
            build_depends.ensure_minimum_version("debhelper", &version);
            actions.push(Action::Deb822(Deb822Action::SetField {
                file: control_rel.clone(),
                paragraph: ParagraphSelector::Source,
                field: "Build-Depends".into(),
                value: build_depends.to_string(),
            }));
        }
    } else {
        // Compat version is set via Build-Depends: debhelper-compat (= N).
        if !has_control {
            return Ok(Vec::new());
        }

        let control = ws.parsed_control()?;
        let Some(source) = control.source() else {
            return Ok(Vec::new());
        };
        let build_depends_str = source.as_deb822().get("Build-Depends").unwrap_or_default();
        let (relations, _) = Relations::parse_relaxed(&build_depends_str, true);

        let debhelper_compat_relations: Vec<_> = relations
            .entries()
            .flat_map(|entry| entry.relations().collect::<Vec<_>>())
            .filter(|rel| rel.try_name().as_deref() == Some("debhelper-compat"))
            .collect();

        if debhelper_compat_relations.len() != 1 {
            return Ok(Vec::new());
        }
        let rel = &debhelper_compat_relations[0];
        let Some(version_constraint) = rel.version() else {
            return Ok(Vec::new());
        };
        if version_constraint.0 != debian_control::relations::VersionConstraint::Equal {
            return Ok(Vec::new());
        }
        current_debhelper_compat_version = match version_constraint.1.to_string().parse() {
            Ok(v) => v,
            Err(_) => return Ok(Vec::new()),
        };
        if current_debhelper_compat_version >= new_debhelper_compat_version {
            return Ok(Vec::new());
        }

        let mut build_depends = source.build_depends().unwrap_or_default();
        let version = Version::from_str(&format!("{}", new_debhelper_compat_version))
            .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
        build_depends.ensure_exact_version("debhelper-compat", &version);
        actions.push(Action::Deb822(Deb822Action::SetField {
            file: control_rel.clone(),
            paragraph: ParagraphSelector::Source,
            field: "Build-Depends".into(),
            value: build_depends.to_string(),
        }));
    }

    // Apply version-specific upgrades.
    for version in (current_debhelper_compat_version + 1)..=new_debhelper_compat_version {
        match version {
            10 => upgrade_to_debhelper_10(ws, &mut actions, &mut transforms)?,
            11 => upgrade_to_debhelper_11(ws, &mut actions, &mut rules_mf, &mut transforms)?,
            12 => upgrade_to_debhelper_12(ws, base_path, &mut rules_mf, &mut transforms)?,
            13 => upgrade_to_debhelper_13(ws, &mut actions, &mut rules_mf, &mut transforms)?,
            _ => {}
        }
    }

    // Drop overrides for tags the rules rewrites just made unused.
    drop_stale_overrides(ws, &mut transforms, &mut actions)?;

    // Emit a single Write for debian/rules if any rules-touching helper
    // opened it AND the resulting content differs from the original.
    if let Some(mf) = rules_mf {
        let mut new_text = mf.to_string();
        // Normalize trailing newlines to match the original file: the
        // makefile-lossless renderer can add a trailing blank line that
        // wasn't in the source after rule removal/insertion.
        let orig_trailing_nls = original_rules_text
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&b| b == b'\n')
            .count();
        let new_trailing_nls = new_text
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&b| b == b'\n')
            .count();
        if new_trailing_nls > orig_trailing_nls {
            let drop = new_trailing_nls - orig_trailing_nls;
            new_text.truncate(new_text.len() - drop);
        }
        if new_text != original_rules_text {
            actions.push(Action::Filesystem(FilesystemAction::Write {
                file: PathBuf::from("debian/rules"),
                content: new_text.into_bytes(),
            }));
        }
    }

    if actions.is_empty() {
        return Ok(Vec::new());
    }

    let kind = if current_debhelper_compat_version < lowest_non_deprecated_compat_level() {
        "deprecated"
    } else {
        "old"
    };

    let description = format!(
        "Package uses {} debhelper compat version {}.",
        kind, current_debhelper_compat_version
    );

    let mut label = format!(
        "Bump debhelper from {} {} to {}.",
        kind, current_debhelper_compat_version, new_debhelper_compat_version
    );
    if !transforms.subitems.is_empty() {
        let mut sorted_transforms: Vec<_> = transforms.subitems.iter().collect();
        sorted_transforms.sort();
        for transform in sorted_transforms {
            label.push_str("\n+ ");
            label.push_str(transform);
        }
    }

    let issue = if current_debhelper_compat_version < lowest_non_deprecated_compat_level() {
        LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            visibility: Some(Visibility::Warning),
            tag: Some("package-uses-deprecated-debhelper-compat-version".to_string()),
            info: Some(current_debhelper_compat_version.to_string()),
        }
    } else {
        LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            visibility: Some(Visibility::Warning),
            tag: Some("package-uses-old-debhelper-compat-version".to_string()),
            info: Some(current_debhelper_compat_version.to_string()),
        }
    };

    Ok(vec![Diagnostic::with_actions(
        issue,
        description,
        label,
        actions,
    )])
}

declare_detector! {
    name: "package-uses-deprecated-debhelper-compat-version",
    tags: ["package-uses-deprecated-debhelper-compat-version", "package-uses-old-debhelper-compat-version"],
    triggers: [
        debian_workspace::Trigger::File("debian/compat"),
        debian_workspace::Trigger::File("debian/rules"),
        debian_workspace::Trigger::Changelog(debian_workspace::ChangelogAspect::Version),
        debian_workspace::Trigger::File("configure"),
        debian_workspace::Trigger::File("configure.ac"),
        debian_workspace::Trigger::File("configure.in"),
        debian_workspace::Trigger::File("Makefile.am"),
        debian_workspace::Trigger::File("meson.build"),
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Source",
            field: "Build-Depends",
        },
        debian_workspace::Trigger::Deb822Field {
            file: "debian/control",
            paragraph_key: "Package",
            field: "Package",
        },
        debian_workspace::Trigger::Glob("debian/*.upstart"),
        debian_workspace::Trigger::Glob("debian/*.tmpfile"),
        debian_workspace::Trigger::File("debian/tmpfile"),
        debian_workspace::Trigger::Glob("debian/*.maintscript"),
    ],
    cost: crate::detector::DetectorCost::Filesystem,
    detect: |ws, prefs| detect(ws, prefs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::Detector;
    use crate::Version as DebVersion;
    use std::fs;
    use tempfile::TempDir;

    fn run_apply(base: &Path, prefs: &FixerPreferences) -> Result<crate::FixerResult, FixerError> {
        let v: DebVersion = "1.0-1".parse().unwrap();
        let adapter = DetectorImpl;
        {
            let ws = debian_workspace::fs_workspace::FsWorkspace::new(
                base,
                Some("test-package".into()),
                Some(v.clone()),
            );
            adapter.apply(&ws, prefs)
        }
    }

    #[test]
    fn test_no_compat_file() {
        let temp_dir = TempDir::new().unwrap();
        let prefs = FixerPreferences::default();
        assert!(matches!(
            run_apply(temp_dir.path(), &prefs),
            Err(FixerError::NoChanges)
        ));
    }

    #[test]
    fn test_upgrade_compat_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(debian_dir.join("compat"), "9\n").unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper (>= 9)\n\nPackage: test-package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());

        run_apply(base_path, &preferences).unwrap();

        let compat_content = fs::read_to_string(debian_dir.join("compat")).unwrap();
        assert!(!compat_content.starts_with("9"));
        assert!(compat_content.trim().parse::<u8>().unwrap() > 9);
    }

    #[test]
    fn test_upgrade_debhelper_compat_build_depends() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper-compat (= 9)\n\nPackage: test-package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());

        run_apply(base_path, &preferences).unwrap();

        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!control_content.contains("debhelper-compat (= 9)"));
    }

    #[test]
    fn test_no_upgrade_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());
        let latest = maximum_debhelper_compat_version("sid");

        fs::write(debian_dir.join("compat"), format!("{}\n", latest)).unwrap();
        fs::write(
            debian_dir.join("control"),
            format!(
                "Source: test-package\nBuild-Depends: debhelper (>= {})\n\nPackage: test-package\n",
                latest
            ),
        )
        .unwrap();

        assert!(matches!(
            run_apply(base_path, &preferences),
            Err(FixerError::NoChanges)
        ));
    }

    fn make_ws(base_path: &Path) -> debian_workspace::fs_workspace::FsWorkspace {
        let v: DebVersion = "1.0-1".parse().unwrap();
        debian_workspace::fs_workspace::FsWorkspace::new(
            base_path.to_path_buf(),
            Some("test".into()),
            Some(v),
        )
    }

    #[test]
    fn test_uses_libexecdir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let ws = make_ws(base_path);

        assert!(!uses_libexecdir(&ws));

        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();
        assert!(!uses_libexecdir(&ws));

        fs::write(
            base_path.join("Makefile.am"),
            "myhelper_PROGRAMS = foo\nmyhelperdir = $(libexecdir)/bar\n",
        )
        .unwrap();
        assert!(uses_libexecdir(&ws));
    }

    /// Helper to drive `upgrade_to_debhelper_12` directly against a
    /// scratch directory, returning the resulting debian/rules content.
    fn run_upgrade_12(base_path: &Path) -> String {
        let ws = make_ws(base_path);
        let mut transforms = Transformations::new();
        let mut rules_mf: Option<Makefile> = None;
        upgrade_to_debhelper_12(&ws, base_path, &mut rules_mf, &mut transforms).unwrap();
        rules_mf.map(|mf| mf.to_string()).unwrap_or_default()
    }

    #[test]
    fn test_upgrade_to_12_adds_libexecdir_override() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();
        fs::write(base_path.join("Makefile.am"), "libexecdir = @libexecdir@\n").unwrap();
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let rules = run_upgrade_12(base_path);
        assert!(rules.contains("override_dh_auto_configure"));
        assert!(rules.contains("--libexecdir=/usr/libexec"));
    }

    #[test]
    fn test_upgrade_to_12_existing_override_no_libexecdir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();
        fs::write(base_path.join("Makefile.am"), "libexecdir = @libexecdir@\n").unwrap();
        fs::write(
            debian_dir.join("rules"),
            "#!/usr/bin/make -f\n\noverride_dh_auto_configure:\n\tdh_auto_configure -- --prefix=/usr\n\n%:\n\tdh $@\n",
        )
        .unwrap();

        let rules = run_upgrade_12(base_path);
        assert!(rules.contains("--libexecdir=/usr/libexec"));
        assert!(rules.contains("--prefix=/usr"));
    }

    #[test]
    fn test_upgrade_to_12_existing_override_with_libexecdir() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();
        fs::write(base_path.join("Makefile.am"), "libexecdir = @libexecdir@\n").unwrap();
        let original = "#!/usr/bin/make -f\n\noverride_dh_auto_configure:\n\tdh_auto_configure -- --libexecdir=/custom/path\n\n%:\n\tdh $@\n";
        fs::write(debian_dir.join("rules"), original).unwrap();

        let rules = run_upgrade_12(base_path);
        assert!(rules.contains("--libexecdir=/custom/path"));
        assert!(!rules.contains("--libexecdir=/usr/libexec"));
    }

    #[test]
    fn test_upgrade_to_12_no_libexecdir_usage() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        fs::write(base_path.join("configure.ac"), "AC_INIT\n").unwrap();
        let original = "#!/usr/bin/make -f\n\n%:\n\tdh $@\n";
        fs::write(debian_dir.join("rules"), original).unwrap();

        // No libexecdir usage → upgrade_to_debhelper_12 should not touch
        // rules. update_rules_for_compat_12 may still open it but won't
        // change anything; the resulting text should equal the original.
        let ws = make_ws(base_path);
        let mut transforms = Transformations::new();
        let mut rules_mf: Option<Makefile> = None;
        upgrade_to_debhelper_12(&ws, base_path, &mut rules_mf, &mut transforms).unwrap();
        let rendered = rules_mf.map(|mf| mf.to_string()).unwrap_or_default();
        // Either rules wasn't opened at all, or its rendering equals
        // the input.
        assert!(rendered.is_empty() || rendered == original);
    }

    /// A compat bump that rewrites `dh_clean -k` to `dh_prep` also drops
    /// the now-unused `dh-clean-k-is-deprecated` override (bug #970174).
    #[test]
    fn test_drops_stale_override_for_rewritten_construct() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(debian_dir.join("source")).unwrap();

        fs::write(debian_dir.join("compat"), "8\n").unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper (>= 8)\n\nPackage: test-package\nArchitecture: any\n",
        )
        .unwrap();
        fs::write(
            debian_dir.join("rules"),
            "%:\n\tdh $@\n\noverride_dh_prep:\n\tdh_clean -k\n",
        )
        .unwrap();
        fs::write(
            debian_dir.join("source/lintian-overrides"),
            "test-package source: dh-clean-k-is-deprecated\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("buster".to_string());

        let result = run_apply(base_path, &preferences).unwrap();
        assert!(result.description.contains(
            "debian/source/lintian-overrides: Drop now-unused override for dh-clean-k-is-deprecated."
        ));

        let rules = fs::read_to_string(debian_dir.join("rules")).unwrap();
        assert!(rules.contains("dh_prep"));
        assert!(!rules.contains("dh_clean -k"));

        // The override file held only the now-unused line, so it is
        // removed entirely once that line is dropped.
        assert!(!debian_dir.join("source/lintian-overrides").exists());
    }

    /// Overrides for tags unrelated to the changes made are left alone.
    #[test]
    fn test_keeps_unrelated_override() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir_all(debian_dir.join("source")).unwrap();

        fs::write(debian_dir.join("compat"), "8\n").unwrap();
        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper (>= 8)\n\nPackage: test-package\nArchitecture: any\n",
        )
        .unwrap();
        fs::write(debian_dir.join("rules"), "%:\n\tdh $@\n").unwrap();
        let override_line = "test-package source: some-other-tag\n";
        fs::write(debian_dir.join("source/lintian-overrides"), override_line).unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("buster".to_string());

        run_apply(base_path, &preferences).unwrap();

        let overrides = fs::read_to_string(debian_dir.join("source/lintian-overrides")).unwrap();
        assert_eq!(overrides, override_line);
    }
}
