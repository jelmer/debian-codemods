use crate::debhelper::detect_debhelper_buildsystem;
use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult, LintianIssue};
use debian_analyzer::control::TemplatedControlEditor;
use debian_analyzer::debhelper::{
    lowest_non_deprecated_compat_level, maximum_debhelper_compat_version,
    read_debhelper_compat_file,
};
use debian_control::lossless::relations::Relations;
use debversion::Version;
use makefile_lossless::{Makefile, Rule};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

fn is_debcargo_package(base_path: &Path) -> bool {
    base_path.join("debian/debcargo.toml").exists()
}

fn autoreconf_disabled(base_path: &Path) -> bool {
    let rules_path = base_path.join("debian/rules");
    let Ok(file) = std::fs::File::open(&rules_path) else {
        return false;
    };

    let mf = match Makefile::read(file) {
        Ok(mf) => mf,
        Err(_) => return false,
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

fn get_current_package_version(base_path: &Path) -> Result<Version, FixerError> {
    let changelog_path = base_path.join("debian/changelog");

    // If no changelog exists, return default version
    if !changelog_path.exists() {
        return Ok("1.0-1".parse().unwrap());
    }

    let contents = fs::read_to_string(&changelog_path)?;
    let changelog = debian_changelog::ChangeLog::read(&mut contents.as_bytes())
        .map_err(|e| FixerError::Other(format!("Failed to parse changelog: {:?}", e)))?;

    let entries: Vec<_> = changelog.iter().collect();
    if let Some(entry) = entries.first() {
        entry
            .version()
            .map(|v| v.clone())
            .ok_or_else(|| FixerError::Other("No version in changelog entry".to_string()))
    } else {
        Err(FixerError::Other("No entries in changelog".to_string()))
    }
}

// Transformation tracking
struct Transformations {
    subitems: HashSet<String>,
}

impl Transformations {
    fn new() -> Self {
        Self {
            subitems: HashSet::new(),
        }
    }

    fn add(&mut self, item: impl Into<String>) {
        self.subitems.insert(item.into());
    }

    fn remove(&mut self, item: &str) {
        self.subitems.remove(item);
    }
}

// Upgrade to debhelper 10
fn upgrade_to_debhelper_10(
    base_path: &Path,
    _transforms: &mut Transformations,
) -> Result<(), FixerError> {
    // dh_installinit will no longer install a file named debian/package as an init script.
    let control_path = base_path.join("debian/control");
    if !control_path.exists() {
        return Ok(());
    }

    let editor = TemplatedControlEditor::open(&control_path)?;
    let debian_dir = base_path.join("debian");

    for binary in editor.binaries() {
        let name = binary
            .as_deb822()
            .get("Package")
            .ok_or(FixerError::NoChanges)?;
        let old_path = debian_dir.join(&name);
        if old_path.is_file() {
            let new_path = debian_dir.join(format!("{}.init", name));
            fs::rename(&old_path, &new_path)?;
            _transforms.add(format!("Rename debian/{} to debian/{}.init.", name, name));
        }
    }

    Ok(())
}

// Upgrade to debhelper 11
fn upgrade_to_debhelper_11(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    upgrade_to_installsystemd(base_path, transforms)?;

    // Drop debian/*.upstart files and add rm_conffile to maintscript
    let debian_dir = base_path.join("debian");
    if let Ok(entries) = fs::read_dir(&debian_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
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

            let file_path = entry.path();
            fs::remove_file(&file_path)?;
            transforms.add(format!("Drop obsolete upstart file {}.", name_str));

            // Add maintscript entry
            let current_version = get_current_package_version(base_path)?;
            let maintscript_path = debian_dir.join(format!("{}.maintscript", package));

            let mut content = if maintscript_path.exists() {
                fs::read_to_string(&maintscript_path)?
            } else {
                String::new()
            };

            let rm_conffile_line = format!(
                "rm_conffile /etc/init/{}.conf {}\n",
                service, current_version
            );

            if !content.contains(&rm_conffile_line) {
                content.push_str(&rm_conffile_line);
                fs::write(&maintscript_path, content)?;
            }
        }
    }

    Ok(())
}

fn upgrade_to_installsystemd(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::open(&rules_path)
        .map_err(|e| FixerError::Other(format!("Failed to open rules: {:?}", e)))?;
    let mf = Makefile::from_reader(&mut file)
        .map_err(|e| FixerError::Other(format!("Failed to read makefile: {:?}", e)))?;
    let mut changed = false;

    // Process all rules directly - mutation methods modify in place
    for mut rule in mf.rules() {
        let targets: Vec<String> = rule.targets().collect();

        // Rename targets using the mutation method
        if targets.contains(&"override_dh_systemd_enable".to_string()) {
            rule.rename_target("override_dh_systemd_enable", "override_dh_installsystemd")
                .map_err(|e| FixerError::Other(format!("Failed to rename target: {:?}", e)))?;
            changed = true;
        }
        if targets.contains(&"override_dh_systemd_start".to_string()) {
            rule.rename_target("override_dh_systemd_start", "override_dh_installsystemd")
                .map_err(|e| FixerError::Other(format!("Failed to rename target: {:?}", e)))?;
            changed = true;
        }

        // Transform recipes using mutation methods
        let recipes: Vec<String> = rule.recipes().collect();
        for (recipe_idx, recipe) in recipes.iter().enumerate() {
            let mut new_recipe = recipe.clone();
            let mut recipe_changed = false;

            // Drop --with=systemd
            if new_recipe.trim_start().starts_with("dh ") {
                let old = new_recipe.clone();
                new_recipe = debian_analyzer::rules::dh_invoke_drop_with(&new_recipe, "systemd");
                if new_recipe != old {
                    transforms.add("Drop --with=systemd, no longer required.".to_string());
                    recipe_changed = true;
                }
            }

            // Replace dh_systemd_enable with dh_installsystemd
            if new_recipe.contains("dh_systemd_enable") {
                new_recipe = new_recipe.replace("dh_systemd_enable", "dh_installsystemd");
                transforms.add(
                    "Use dh_installsystemd rather than deprecated dh_systemd_enable.".to_string(),
                );
                recipe_changed = true;
            }

            // Replace dh_systemd_start with dh_installsystemd
            if new_recipe.contains("dh_systemd_start") {
                new_recipe = new_recipe.replace("dh_systemd_start", "dh_installsystemd");
                transforms.add(
                    "Use dh_installsystemd rather than deprecated dh_systemd_start.".to_string(),
                );
                recipe_changed = true;
            }

            if recipe_changed {
                rule.replace_command(recipe_idx, &new_recipe);
                changed = true;
            }
        }
    }

    if changed {
        fs::write(&rules_path, mf.to_string())?;
    }

    Ok(())
}

// Upgrade to debhelper 12
fn upgrade_to_debhelper_12(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    update_rules_for_compat_12(base_path, transforms)?;
    Ok(())
}

fn update_rules_for_compat_12(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::open(&rules_path)
        .map_err(|e| FixerError::Other(format!("Failed to open rules: {:?}", e)))?;
    let mut mf = Makefile::from_reader(&mut file)
        .map_err(|e| FixerError::Other(format!("Failed to read makefile: {:?}", e)))?;

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
                            log::debug!("Detected python_distutils buildsystem, upgrading to pybuild");
                            new_recipe =
                                new_recipe.trim_end().to_string() + " --buildsystem=pybuild";
                            transforms.add(
                                "Replace python_distutils buildsystem with pybuild.".to_string(),
                            );
                            pybuild_upgraded = true;
                        }
                        Ok(Some(buildsystem)) => {
                            log::debug!("Detected buildsystem: {}, not upgrading to pybuild", buildsystem);
                        }
                        Ok(None) => {
                            log::debug!("No buildsystem detected");
                        }
                        Err(e) => {
                            log::warn!("Failed to detect buildsystem: {}", e);
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
                    let dh_cmd = before.trim_start().split_whitespace().next().unwrap_or("");
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
            changed = true;
        }
    }

    if changed {
        fs::write(&rules_path, mf.to_string())?;
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
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    // Rename debian/*.tmpfile to debian/*.tmpfiles
    let control_path = base_path.join("debian/control");
    if control_path.exists() {
        let editor = TemplatedControlEditor::open(&control_path)?;
        let debian_dir = base_path.join("debian");

        for binary in editor.binaries() {
            let name = binary
                .as_deb822()
                .get("Package")
                .ok_or(FixerError::NoChanges)?;
            let tmpfile_path = debian_dir.join(format!("{}.tmpfile", name));
            if tmpfile_path.is_file() {
                let tmpfiles_path = debian_dir.join(format!("{}.tmpfiles", name));
                fs::rename(&tmpfile_path, &tmpfiles_path)?;
                transforms.add(format!(
                    "Rename debian/{}.tmpfile to debian/{}.tmpfiles.",
                    name, name
                ));
            }
        }

        // Also check for generic tmpfile
        let tmpfile_path = debian_dir.join("tmpfile");
        if tmpfile_path.is_file() {
            let tmpfiles_path = debian_dir.join("tmpfiles");
            fs::rename(&tmpfile_path, &tmpfiles_path)?;
            transforms.add("Rename debian/tmpfile to debian/tmpfiles.".to_string());
        }
    }

    // Drop --fail-missing from dh_missing calls
    drop_dh_missing_fail(base_path, transforms)?;

    // Remove DEB_BUILD_OPTIONS nocheck wrapper from override_dh_auto_test
    remove_nocheck_wrapper(base_path, transforms)?;

    Ok(())
}

fn drop_dh_missing_fail(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::open(&rules_path)
        .map_err(|e| FixerError::Other(format!("Failed to open rules: {:?}", e)))?;
    let mf = Makefile::from_reader(&mut file)
        .map_err(|e| FixerError::Other(format!("Failed to read makefile: {:?}", e)))?;

    let mut changed = false;
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
                changed = true;
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
                changed = true;
            }
        }
    }

    if changed {
        fs::write(&rules_path, mf.to_string())?;
    }

    Ok(())
}

fn remove_nocheck_wrapper(
    base_path: &Path,
    transforms: &mut Transformations,
) -> Result<(), FixerError> {
    let rules_path = base_path.join("debian/rules");
    if !rules_path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::open(&rules_path)?;
    let mf = Makefile::from_reader(&mut file)
        .map_err(|e| FixerError::Other(format!("Failed to read makefile: {:?}", e)))?;

    let mut changed = false;

    for rule in mf.rules() {
        let targets: Vec<String> = rule.targets().collect();
        if !targets.contains(&"override_dh_auto_test".to_string()) {
            continue;
        }

        // Iterate through rule items to find conditionals
        for item in rule.items() {
            if let makefile_lossless::RuleItem::Conditional(mut cond) = item {
                // Check if it's a DEB_BUILD_OPTIONS nocheck wrapper
                let cond_str = cond.to_string();
                if cond_str.contains("ifeq (,$(filter nocheck,$(DEB_BUILD_OPTIONS)))") {
                    transforms.add(
                        "Drop check for DEB_BUILD_OPTIONS containing \"nocheck\", since debhelper now does this.".to_string(),
                    );

                    // Unwrap the conditional, keeping its contents
                    cond.unwrap().map_err(|e| {
                        FixerError::Other(format!("Failed to unwrap conditional: {:?}", e))
                    })?;
                    changed = true;
                }
            }
        }
    }

    if changed {
        fs::write(&rules_path, mf.to_string())?;
    }

    Ok(())
}

pub fn run(base_path: &Path, preferences: &FixerPreferences) -> Result<FixerResult, FixerError> {
    // Get the compat_release from preferences, defaulting to "sid"
    let compat_release = preferences.compat_release.as_deref().unwrap_or("sid");

    let mut new_debhelper_compat_version = maximum_debhelper_compat_version(compat_release);

    // Check if the package uses CDBS
    let uses_cdbs = debian_analyzer::rules::check_cdbs(&base_path.join("debian/rules"));
    if uses_cdbs {
        // cdbs doesn't appear to support debhelper 11 or 12 just yet..
        new_debhelper_compat_version = new_debhelper_compat_version.min(10);
    }

    // Check if autoreconf is disabled
    if autoreconf_disabled(base_path) {
        let configure_path = base_path.join("configure");
        if configure_path.exists() {
            if let Ok(contents) = fs::read_to_string(&configure_path) {
                if !contents.contains("runstatedir") {
                    new_debhelper_compat_version = new_debhelper_compat_version.min(10);
                }
            }
        }
    }

    let compat_path = base_path.join("debian/compat");
    let control_path = base_path.join("debian/control");

    let current_debhelper_compat_version: u8;
    let mut transforms = Transformations::new();

    if compat_path.exists() {
        // Package currently stores compat version in debian/compat
        current_debhelper_compat_version = match read_debhelper_compat_file(&compat_path)? {
            Some(v) => v,
            None => return Err(FixerError::NoChanges),
        };

        if current_debhelper_compat_version >= new_debhelper_compat_version {
            return Err(FixerError::NoChanges);
        }

        // Update debian/compat
        fs::write(&compat_path, format!("{}\n", new_debhelper_compat_version))?;

        // Update Build-Depends in debian/control
        let editor = TemplatedControlEditor::open(&control_path)?;
        if let Some(mut source) = editor.source() {
            let mut build_depends = source.build_depends().unwrap_or_default();
            let version = Version::from_str(&format!("{}~", new_debhelper_compat_version))
                .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
            build_depends.ensure_minimum_version("debhelper", &version);
            source.set_build_depends(&build_depends);
        }
        editor.commit()?;
    } else {
        // Assume that the compat version is set in Build-Depends
        if !control_path.exists() {
            // debcargo packages just use the latest version and don't store debhelper
            // version explicitly.
            if is_debcargo_package(base_path) {
                return Err(FixerError::NoChanges);
            }
            return Err(FixerError::NoChanges);
        }

        let editor = TemplatedControlEditor::open(&control_path)?;

        let mut source = editor.source().ok_or(FixerError::NoChanges)?;
        let build_depends_str = source.as_deb822().get("Build-Depends").unwrap_or_default();

        // Parse the relations to find debhelper-compat
        let (relations, _) = Relations::parse_relaxed(&build_depends_str, true);

        let debhelper_compat_relations: Vec<_> = relations
            .entries()
            .flat_map(|entry| entry.relations().collect::<Vec<_>>())
            .filter(|rel| rel.name() == "debhelper-compat")
            .collect();

        if debhelper_compat_relations.is_empty() {
            return Err(FixerError::NoChanges);
        }

        if debhelper_compat_relations.len() > 1 {
            // Not sure how to deal with this
            return Err(FixerError::NoChanges);
        }

        let rel = &debhelper_compat_relations[0];
        let version_constraint = rel.version().ok_or(FixerError::NoChanges)?;

        if version_constraint.0 != debian_control::relations::VersionConstraint::Equal {
            // Not sure how to deal with this
            return Err(FixerError::NoChanges);
        }

        current_debhelper_compat_version = version_constraint
            .1
            .to_string()
            .parse()
            .map_err(|_| FixerError::NoChanges)?;

        if current_debhelper_compat_version >= new_debhelper_compat_version {
            return Err(FixerError::NoChanges);
        }

        // Update the Build-Depends
        let mut build_depends = source.build_depends().unwrap_or_default();
        let version = Version::from_str(&format!("{}", new_debhelper_compat_version))
            .map_err(|e| FixerError::Other(format!("Failed to parse version: {:?}", e)))?;
        build_depends.ensure_exact_version("debhelper-compat", &version);
        source.set_build_depends(&build_depends);

        editor.commit()?;
    }

    // Apply version-specific upgrades
    for version in (current_debhelper_compat_version + 1)..=new_debhelper_compat_version {
        match version {
            10 => upgrade_to_debhelper_10(base_path, &mut transforms)?,
            11 => upgrade_to_debhelper_11(base_path, &mut transforms)?,
            12 => upgrade_to_debhelper_12(base_path, &mut transforms)?,
            13 => upgrade_to_debhelper_13(base_path, &mut transforms)?,
            _ => {}
        }
    }

    let kind = if current_debhelper_compat_version < lowest_non_deprecated_compat_level() {
        "deprecated"
    } else {
        "old"
    };

    let mut description = format!(
        "Bump debhelper from {} {} to {}.",
        kind, current_debhelper_compat_version, new_debhelper_compat_version
    );

    // Add transform details to description
    if !transforms.subitems.is_empty() {
        let mut sorted_transforms: Vec<_> = transforms.subitems.iter().collect();
        sorted_transforms.sort();
        for transform in sorted_transforms {
            description.push_str("\n+ ");
            description.push_str(transform);
        }
    }

    let mut result = FixerResult::builder(description);

    if current_debhelper_compat_version < lowest_non_deprecated_compat_level() {
        result = result.fixed_issue(LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            tag: Some("package-uses-deprecated-debhelper-compat-version".to_string()),
            info: Some(current_debhelper_compat_version.to_string()),
        });
    } else {
        result = result.fixed_issue(LintianIssue {
            package: None,
            package_type: Some(crate::PackageType::Source),
            tag: Some("package-uses-old-debhelper-compat-version".to_string()),
            info: Some(current_debhelper_compat_version.to_string()),
        });
    }

    Ok(result.build())
}

declare_fixer! {
    name: "package-uses-deprecated-debhelper-compat-version",
    tags: ["package-uses-deprecated-debhelper-compat-version", "package-uses-old-debhelper-compat-version"],
    apply: |basedir, _package, _version, preferences| {
        run(basedir, preferences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_no_compat_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let preferences = FixerPreferences::default();

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }

    #[test]
    fn test_upgrade_compat_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create a compat file with version 9
        fs::write(debian_dir.join("compat"), "9\n").unwrap();

        // Create a control file
        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper (>= 9)\n\nPackage: test-package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        // Check that compat was updated
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

        // Create a control file with debhelper-compat
        fs::write(
            debian_dir.join("control"),
            "Source: test-package\nBuild-Depends: debhelper-compat (= 9)\n\nPackage: test-package\n",
        )
        .unwrap();

        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());

        let result = run(base_path, &preferences);
        assert!(result.is_ok());

        // Check that control was updated
        let control_content = fs::read_to_string(debian_dir.join("control")).unwrap();
        assert!(!control_content.contains("debhelper-compat (= 9)"));
    }

    #[test]
    fn test_no_upgrade_needed() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();
        let debian_dir = base_path.join("debian");
        fs::create_dir(&debian_dir).unwrap();

        // Create a compat file with the maximum version for sid
        let mut preferences = FixerPreferences::default();
        preferences.compat_release = Some("sid".to_string());
        let latest = maximum_debhelper_compat_version("sid");

        fs::write(debian_dir.join("compat"), format!("{}\n", latest)).unwrap();

        // Create a control file
        fs::write(
            debian_dir.join("control"),
            format!(
                "Source: test-package\nBuild-Depends: debhelper (>= {})\n\nPackage: test-package\n",
                latest
            ),
        )
        .unwrap();

        let result = run(base_path, &preferences);
        assert!(matches!(result, Err(FixerError::NoChanges)));
    }
}
