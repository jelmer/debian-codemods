use quote::quote;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("fixer_tests.rs");

    let mut dest = fs::File::create(dest_path).unwrap();

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let test_dir = Path::new(&manifest_dir).join("tests");

    // Generate Debian control field names
    generate_debian_control_fields(&out_dir);

    // Generate obsolete sites list
    generate_obsolete_sites(&out_dir);

    // Read test directories to discover test names
    let test_dirs = fs::read_dir(test_dir).unwrap();

    // Sort directory entries for deterministic ordering
    let mut test_dir_entries: Vec<_> = test_dirs.collect::<Result<_, _>>().unwrap();
    test_dir_entries.sort_by_key(|entry| entry.file_name());

    for test_dir_entry in test_dir_entries {
        if !test_dir_entry.file_type().unwrap().is_dir() {
            continue;
        }

        let fixer_name = test_dir_entry.file_name().into_string().unwrap();

        // Discover the tests for this fixer
        let tests = fs::read_dir(test_dir_entry.path()).unwrap();

        // Sort test entries for deterministic ordering
        let mut test_entries: Vec<_> = tests.collect::<Result<_, _>>().unwrap();
        test_entries.sort_by_key(|entry| entry.file_name());

        dest.write_all("#[allow(non_snake_case)]\n".as_bytes())
            .unwrap();
        let module_name = fixer_name.replace(['-', '.'], "_");
        dest.write_all(format!("mod {} {{\n", module_name).as_bytes())
            .unwrap();

        for test in test_entries {
            if !test.file_type().unwrap().is_dir() {
                continue;
            }
            let test_name = test.file_name().into_string().unwrap();
            let test_name = test_name.trim_end_matches(".desc");

            let test_path = test.path().to_str().unwrap().to_string();

            let fn_name = quote::format_ident!("test_{}", test_name.replace(['-', '.'], "_"));

            let test = quote! {
                #[test]
                fn #fn_name() {
                    crate::fixer_tests::run_fixer_testcase(#fixer_name, #test_name, std::path::Path::new(#test_path));
                }
            };

            // Write the test to the output file
            dest.write_all(test.to_string().as_bytes()).unwrap();
        }

        dest.write_all("}\n".as_bytes()).unwrap();
    }

    // Generate renamed tags map
    generate_renamed_tags_map(&out_dir);

    // Generate SPDX license data
    generate_spdx_data(&out_dir);

    // rebuild if build.rs or tests directory changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=tests");
    println!("cargo:rerun-if-changed=renamed-tags.json");
    println!("cargo:rerun-if-changed=/usr/share/lintian/data/obsolete-sites/obsolete-sites");
    println!("cargo:rerun-if-changed=../spdx.json");
}

fn generate_renamed_tags_map(out_dir: &std::ffi::OsStr) {
    let dest_path = Path::new(out_dir).join("renamed_tags.rs");

    // Read and parse the JSON file
    let json_content =
        fs::read_to_string("renamed-tags.json").expect("Failed to read renamed-tags.json");

    let renames: std::collections::HashMap<String, String> =
        serde_json::from_str(&json_content).expect("Failed to parse renamed-tags.json");

    // Generate Rust code for the hashmap
    let mut code = String::new();
    code.push_str("/// Get the mapping of old lintian tag names to their current names.\n");
    code.push_str(
        "pub fn get_renamed_tags() -> indexmap::IndexMap<&'static str, &'static str> {\n",
    );
    code.push_str("    let mut map = indexmap::IndexMap::new();\n");

    // Sort the keys to ensure deterministic ordering
    let mut sorted_renames: Vec<_> = renames.into_iter().collect();
    sorted_renames.sort_by(|a, b| a.0.cmp(&b.0));

    for (old_tag, new_tag) in sorted_renames {
        // Escape any quotes in the strings
        let old_tag = old_tag.replace('\"', "\\\"");
        let new_tag = new_tag.replace('\"', "\\\"");
        code.push_str(&format!(
            "    map.insert(\"{}\", \"{}\");\n",
            old_tag, new_tag
        ));
    }

    code.push_str("    map\n");
    code.push_str("}\n");

    fs::write(&dest_path, code).unwrap();
}

fn generate_debian_control_fields(out_dir: &std::ffi::OsStr) {
    let dest_path = Path::new(out_dir).join("debian_control_fields.rs");

    let mut code = String::new();

    // Read from system lintian data files
    let source_fields = read_field_list_with_vendor("/usr/share/lintian/data/common/source-fields")
        .or_else(|| read_field_list_with_vendor("/usr/share/lintian/data/fields/source-fields"))
        .expect("Could not find Debian source fields data file");

    let binary_fields = read_field_list_with_vendor("/usr/share/lintian/data/fields/binary-fields")
        .expect("Could not find Debian binary fields data file");

    code.push_str("#[derive(Clone, Debug)]\n");
    code.push_str("pub struct FieldEntry {\n");
    code.push_str("    pub name: &'static str,\n");
    code.push_str("    pub excluded_vendor: Option<&'static str>,\n");
    code.push_str("}\n\n");

    // Generate source fields data
    code.push_str("pub const DEBIAN_SOURCE_FIELDS: &[FieldEntry] = &[\n");
    for (field, vendor_constraint) in &source_fields {
        if let Some(excluded_vendor) = vendor_constraint {
            code.push_str(&format!(
                "    FieldEntry {{ name: \"{}\", excluded_vendor: Some(\"{}\") }},\n",
                field.replace('\"', "\\\""),
                excluded_vendor.replace('\"', "\\\"")
            ));
        } else {
            code.push_str(&format!(
                "    FieldEntry {{ name: \"{}\", excluded_vendor: None }},\n",
                field.replace('\"', "\\\"")
            ));
        }
    }
    code.push_str("];\n\n");

    // Generate binary fields data
    code.push_str("pub const DEBIAN_BINARY_FIELDS: &[FieldEntry] = &[\n");
    for (field, vendor_constraint) in &binary_fields {
        if let Some(excluded_vendor) = vendor_constraint {
            code.push_str(&format!(
                "    FieldEntry {{ name: \"{}\", excluded_vendor: Some(\"{}\") }},\n",
                field.replace('\"', "\\\""),
                excluded_vendor.replace('\"', "\\\"")
            ));
        } else {
            code.push_str(&format!(
                "    FieldEntry {{ name: \"{}\", excluded_vendor: None }},\n",
                field.replace('\"', "\\\"")
            ));
        }
    }
    code.push_str("];\n\n");

    // Generate helper functions
    code.push_str("pub fn known_debian_source_fields(vendor: &str) -> std::collections::HashSet<&'static str> {\n");
    code.push_str("    DEBIAN_SOURCE_FIELDS.iter()\n");
    code.push_str("        .filter(|entry| entry.excluded_vendor.map_or(true, |v| v != vendor))\n");
    code.push_str("        .map(|entry| entry.name)\n");
    code.push_str("        .collect()\n");
    code.push_str("}\n\n");

    code.push_str("pub fn known_debian_binary_fields(vendor: &str) -> std::collections::HashSet<&'static str> {\n");
    code.push_str("    DEBIAN_BINARY_FIELDS.iter()\n");
    code.push_str("        .filter(|entry| entry.excluded_vendor.map_or(true, |v| v != vendor))\n");
    code.push_str("        .map(|entry| entry.name)\n");
    code.push_str("        .collect()\n");
    code.push_str("}\n");

    fs::write(&dest_path, code).unwrap();
}

fn read_field_list_with_vendor(path: &str) -> Option<Vec<(String, Option<String>)>> {
    fs::read_to_string(path).ok().map(|content| {
        let mut current_vendor_exclusion: Option<String> = None;
        let mut fields = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Handle vendor-specific directives
            if let Some(stripped) = trimmed.strip_prefix("@if-vendor-is-not ") {
                current_vendor_exclusion = Some(stripped.trim().to_string());
                continue;
            }
            if trimmed == "@endif" {
                current_vendor_exclusion = None;
                continue;
            }

            // Add field with current vendor constraint
            fields.push((trimmed.to_string(), current_vendor_exclusion.clone()));
        }

        fields
    })
}

fn generate_obsolete_sites(out_dir: &std::ffi::OsStr) {
    let dest_path = Path::new(out_dir).join("obsolete_sites.rs");

    let content = fs::read_to_string("/usr/share/lintian/data/obsolete-sites/obsolete-sites")
        .expect("Could not find obsolete sites data file");

    let mut code = String::new();
    code.push_str("pub static OBSOLETE_SITES: &[&str] = &[\n");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        code.push_str(&format!("    \"{}\",\n", trimmed.replace('\"', "\\\"")));
    }

    code.push_str("];\n\n");
    code.push_str("pub fn is_obsolete_site(hostname: &str) -> bool {\n");
    code.push_str("    OBSOLETE_SITES.iter().any(|&site| {\n");
    code.push_str("        hostname == site || hostname.ends_with(&format!(\".{}\", site))\n");
    code.push_str("    })\n");
    code.push_str("}\n");

    fs::write(&dest_path, code).unwrap();
}

fn generate_spdx_data(out_dir: &std::ffi::OsStr) {
    let dest_path = Path::new(out_dir).join("spdx_licenses.rs");

    // Read the spdx.json file from the parent directory
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let spdx_path = Path::new(&manifest_dir).join("..").join("spdx.json");

    let json_content = fs::read_to_string(&spdx_path)
        .expect("Failed to read spdx.json - make sure it exists in the parent directory");

    #[derive(serde::Deserialize)]
    struct SpdxLicense {
        name: String,
    }

    #[derive(serde::Deserialize)]
    struct SpdxData {
        licenses: std::collections::HashMap<String, SpdxLicense>,
    }

    let spdx_data: SpdxData =
        serde_json::from_str(&json_content).expect("Failed to parse spdx.json");

    // Deprecated SPDX license IDs that should be deprioritized when deduplicating
    let deprecated_ids: std::collections::HashSet<&str> = [
        "AGPL-1.0",
        "AGPL-3.0",
        "GFDL-1.1",
        "GFDL-1.2",
        "GFDL-1.3",
        "GPL-1.0",
        "GPL-1.0+",
        "GPL-2.0",
        "GPL-2.0+",
        "GPL-3.0",
        "GPL-3.0+",
        "LGPL-2.0",
        "LGPL-2.0+",
        "LGPL-2.1",
        "LGPL-2.1+",
        "LGPL-3.0",
        "LGPL-3.0+",
    ]
    .iter()
    .copied()
    .collect();

    // Collect and sort license IDs
    let mut license_ids: Vec<&String> = spdx_data.licenses.keys().collect();
    license_ids.sort();

    // Generate the license ID array
    let license_id_literals = license_ids.iter().map(|id| id.as_str());
    let spdx_ids_code = quote! {
        pub static SPDX_LICENSE_IDS: &[&str] = &[
            #(#license_id_literals),*
        ];
    };

    // Generate the license name to ID mapping
    let mut name_to_id_pairs: Vec<(String, &str)> = spdx_data
        .licenses
        .iter()
        .map(|(id, license)| (license.name.to_lowercase(), id.as_str()))
        .collect();

    // Sort by name, then prioritize non-deprecated IDs, then by ID for determinism
    name_to_id_pairs.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| {
                // Non-deprecated (false) should come before deprecated (true)
                deprecated_ids
                    .contains(a.1)
                    .cmp(&deprecated_ids.contains(b.1))
            })
            .then_with(|| a.1.cmp(b.1))
    });

    // Deduplicate by name, keeping the first (non-deprecated when available)
    name_to_id_pairs.dedup_by(|a, b| a.0 == b.0);

    let names: Vec<&str> = name_to_id_pairs
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let ids: Vec<&str> = name_to_id_pairs.iter().map(|(_, id)| *id).collect();

    let renames_code = quote! {
        pub fn get_spdx_license_renames() -> indexmap::IndexMap<&'static str, &'static str> {
            let mut map = indexmap::IndexMap::new();
            #(
                map.insert(#names, #ids);
            )*
            map
        }
    };

    let full_code = quote! {
        #spdx_ids_code

        #renames_code
    };

    fs::write(&dest_path, full_code.to_string()).unwrap();
}
