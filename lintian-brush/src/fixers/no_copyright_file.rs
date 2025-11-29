use crate::{declare_fixer, FixerError, FixerPreferences, FixerResult};
use std::path::Path;

mod decopy {
    use pyo3::prelude::*;
    use pyo3::types::PyList;
    use std::path::Path;

    #[derive(Debug)]
    pub enum Error {
        Python(pyo3::PyErr),
        NotAvailable,
    }

    impl From<pyo3::PyErr> for Error {
        fn from(err: pyo3::PyErr) -> Self {
            Error::Python(err)
        }
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Error::Python(err) => write!(f, "Python error: {}", err),
                Error::NotAvailable => write!(f, "decopy module not available"),
            }
        }
    }

    impl std::error::Error for Error {}

    #[derive(Debug, Clone)]
    pub struct FileGroup {
        pub files: Vec<String>,
        pub copyrights: Vec<String>,
        pub license: String,
        pub comments: Option<String>,
    }

    /// Scan the source tree using decopy and return file groups with their metadata
    pub fn scan_tree(base_path: &Path) -> Result<(Vec<FileGroup>, Vec<String>), Error> {
        Python::attach(|py| {
            // Try to import decopy
            if py.import("decopy").is_err() {
                return Err(Error::NotAvailable);
            }

            // Import required modules
            let cmdoptions = py.import("decopy.cmdoptions")?;
            let dep5 = py.import("decopy.dep5")?;
            let tree = py.import("decopy.tree")?;
            let datatypes = py.import("decopy.datatypes")?;

            // Convert to absolute path
            let abs_path = std::fs::canonicalize(base_path)
                .map_err(|e| Error::Python(pyo3::exceptions::PyIOError::new_err(e.to_string())))?;
            let abs_path_str = abs_path.to_str().unwrap();

            let result = (|| -> Result<(Vec<FileGroup>, Vec<String>), Error> {
                // Process options
                let root_arg = format!("--root={}", abs_path_str);
                let output_arg = format!("--output={}/debian/copyright", abs_path_str);
                let args = PyList::new(
                    py,
                    [
                        root_arg.as_str(),
                        "--no-progress",
                        "--mode=full",
                        output_arg.as_str(),
                    ],
                )?;
                let options = cmdoptions.getattr("process_options")?.call1((args,))?;

                // Build file tree
                let root_info = tree.getattr("RootInfo")?;
                let filetree = root_info.call_method1("build", (&options,))?;

                // Build copyright
                let copyright_class = dep5.getattr("Copyright")?;
                let copyright_ = copyright_class.call_method1("build", (&filetree, &options))?;

                // Process
                copyright_.call_method1("process", (&filetree,))?;
                filetree.call_method1("process", (&options,))?;

                // Get groups dictionary
                let groups = copyright_.call_method1("get_group_dict", (&options,))?;

                // Get DirInfo and Group classes
                let dir_info = tree.getattr("DirInfo")?;
                let group_class = dep5.getattr("Group")?;

                // Process ungrouped files (matching Python logic)
                let builtins = py.import("builtins")?;
                let fileinfos: Vec<Py<PyAny>> =
                    builtins.call_method1("list", (filetree,))?.extract()?;
                for fileinfo_py in fileinfos {
                    let fileinfo = fileinfo_py.bind(py);

                    // Check if already has group
                    let has_group = fileinfo.getattr("group")?;
                    if !has_group.is_none() {
                        continue;
                    }

                    // Skip directories
                    if fileinfo.is_instance(&dir_info)? {
                        continue;
                    }

                    // Get or create group
                    let file_key = fileinfo.call_method1("get_group_key", (&options,))?;

                    // Use setdefault to get or create group
                    let group = groups.call_method1(
                        "setdefault",
                        (&file_key, group_class.call1((&file_key,))?),
                    )?;

                    group.call_method1("add_file", (fileinfo,))?;
                    fileinfo.setattr("group", &group)?;
                }

                // Collect file groups
                let mut file_groups = Vec::new();
                let mut all_licenses = std::collections::HashSet::new();

                // Get items and sort them - use Python's sorted() to maintain correct ordering
                // We need to sort using Python's sorted() with the sort_key function, since
                // sort keys are complex tuples that can't be easily extracted to Rust
                let items_list: Vec<(Py<PyAny>, Py<PyAny>)> = builtins
                    .call_method1("list", (groups.call_method0("items")?,))?
                    .extract()?;

                // Sort the items using Python comparison of sort keys
                let mut sorted_items = items_list;
                sorted_items.sort_by_cached_key(|(_, group)| {
                    group
                        .bind(py)
                        .call_method1("sort_key", (&options,))
                        .ok()
                        .map(|k| k.to_string())
                        .unwrap_or_default()
                });

                for (_key, group) in sorted_items {
                    let group = group.bind(py);

                    // Check if copyright block is valid
                    if !group.call_method0("copyright_block_valid")?.is_truthy()? {
                        continue;
                    }

                    // Collect licenses
                    let group_licenses = group.getattr("licenses")?;
                    let keys: Vec<String> = builtins
                        .call_method1("list", (group_licenses.call_method0("keys")?,))?
                        .extract()?;
                    for key in keys {
                        all_licenses.insert(key);
                    }

                    // Get files
                    let files = if options.getattr("glob")?.is_truthy()? {
                        group.getattr("files")?.call_method0("get_patterns")?
                    } else {
                        group.getattr("files")?.call_method0("sorted_members")?
                    };
                    let files_list: Vec<String> =
                        builtins.call_method1("list", (files,))?.extract()?;

                    // Get copyright holders
                    let copyrights = if group.getattr("copyrights")?.is_truthy()? {
                        let copyrights_obj = group.getattr("copyrights")?;
                        let sorted = copyrights_obj.call_method0("sorted_members")?;
                        let sorted_list = builtins.call_method1("list", (sorted,))?;
                        sorted_list.extract()?
                    } else {
                        vec!["Unknown".to_string()]
                    };

                    // Get license
                    let license = group.getattr("license")?.extract::<String>()?;

                    // Get comments
                    let comments_obj = group.call_method0("get_comments")?;
                    let comments = if comments_obj.is_none() {
                        None
                    } else {
                        let comment_str: String = comments_obj.extract()?;
                        if comment_str.is_empty() {
                            None
                        } else {
                            Some(comment_str)
                        }
                    };

                    file_groups.push(FileGroup {
                        files: files_list,
                        copyrights,
                        license,
                        comments,
                    });
                }

                // Get license names using decopy's License.get()
                let decopy_license = datatypes.getattr("License")?;
                let mut license_names = Vec::new();
                let mut sorted_licenses: Vec<String> = all_licenses.into_iter().collect();
                sorted_licenses.sort();

                for license_key in sorted_licenses {
                    let license_ = decopy_license.call_method1("get", (license_key,))?;
                    let license_name = license_.getattr("name")?.extract::<String>()?;
                    license_names.push(license_name);
                }

                Ok((file_groups, license_names))
            })();

            result
        })
    }
}

pub fn run(
    base_path: &Path,
    _package_name: &str,
    preferences: &FixerPreferences,
) -> Result<FixerResult, FixerError> {
    let copyright_path = base_path.join("debian/copyright");

    // Check if copyright file already exists
    if copyright_path.exists() {
        return Err(FixerError::NoChanges);
    }

    // Check minimum certainty
    let certainty = crate::Certainty::Possible;
    if !crate::certainty_sufficient(certainty, preferences.minimum_certainty) {
        return Err(FixerError::NotCertainEnough(
            certainty,
            preferences.minimum_certainty,
            vec![],
        ));
    }

    use debian_copyright::lossless::Copyright;
    use debian_copyright::License;

    // Scan the tree using decopy
    let (file_groups, license_names) = match decopy::scan_tree(base_path) {
        Ok(result) => result,
        Err(decopy::Error::NotAvailable) => {
            return Err(FixerError::Other(
                "decopy Python module not available".to_string(),
            ))
        }
        Err(decopy::Error::Python(err)) => {
            return Err(FixerError::Other(format!("Error running decopy: {}", err)))
        }
    };

    // Create a new copyright file
    let mut copyright = Copyright::new();

    // Add files paragraphs
    for group in file_groups {
        let files_refs: Vec<&str> = group.files.iter().map(|s| s.as_str()).collect();
        let copyrights_refs: Vec<&str> = group.copyrights.iter().map(|s| s.as_str()).collect();
        let license = License::Name(group.license.clone());

        let mut files_para = copyright.add_files(&files_refs, &copyrights_refs, &license);

        // Set comment if present
        if let Some(comment) = group.comments {
            files_para.set_comment(&comment);
        }
    }

    // Add license paragraphs
    for license_name in license_names {
        let license = License::Name(license_name);
        let mut license_para = copyright.add_license(&license);
        license_para.set_comment("Add the corresponding license text here");
    }

    // Write to file
    std::fs::write(&copyright_path, copyright.to_string())?;

    Ok(FixerResult::builder("Create a debian/copyright file.")
        .certainty(certainty)
        .fixed_tag("no-copyright-file")
        .build())
}

declare_fixer! {
    name: "no-copyright-file",
    tags: ["no-copyright-file"],
    apply: |basedir, package, _version, preferences| {
        run(basedir, package, preferences)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_debian_copyright_field_order() {
        use debian_copyright::lossless::Copyright;
        use debian_copyright::License;

        let mut copyright = Copyright::new();

        // Add files paragraph
        let files = vec!["*"];
        let copyrights = vec!["Unknown"];
        let license = License::Name("GPL-2+".to_string());

        let mut para = copyright.add_files(&files, &copyrights, &license);
        para.set_comment("Test comment");

        let output = copyright.to_string();
        println!("Generated copyright file:\n{}", output);

        // Check field order
        let lines: Vec<&str> = output.lines().collect();
        let mut files_idx = None;
        let mut copyright_idx = None;
        let mut license_idx = None;
        let mut comment_idx = None;

        for (idx, line) in lines.iter().enumerate() {
            if line.starts_with("Files:") {
                files_idx = Some(idx);
            } else if line.starts_with("Copyright:") {
                copyright_idx = Some(idx);
            } else if line.starts_with("License:") && !line.contains("format") {
                license_idx = Some(idx);
            } else if line.starts_with("Comment:") {
                comment_idx = Some(idx);
            }
        }

        println!(
            "Field positions - Files: {:?}, Copyright: {:?}, License: {:?}, Comment: {:?}",
            files_idx, copyright_idx, license_idx, comment_idx
        );

        // Verify correct order: Files < Copyright < License < Comment
        assert!(files_idx.is_some(), "Files field not found");
        assert!(copyright_idx.is_some(), "Copyright field not found");
        assert!(license_idx.is_some(), "License field not found");

        let f = files_idx.unwrap();
        let c = copyright_idx.unwrap();
        let l = license_idx.unwrap();

        assert!(f < c, "Files ({}) should come before Copyright ({})", f, c);
        assert!(
            c < l,
            "Copyright ({}) should come before License ({})",
            c,
            l
        );

        if let Some(com) = comment_idx {
            assert!(
                l < com,
                "License ({}) should come before Comment ({})",
                l,
                com
            );
        }
    }
}
