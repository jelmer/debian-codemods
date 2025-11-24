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

    for test_dir_entry in test_dirs {
        let test_dir_entry = test_dir_entry.unwrap();
        if !test_dir_entry.file_type().unwrap().is_dir() {
            continue;
        }

        let fixer_name = test_dir_entry.file_name().into_string().unwrap();

        // Discover the tests for this fixer
        let tests = fs::read_dir(test_dir_entry.path()).unwrap();

        dest.write_all("#[allow(non_snake_case)]\n".as_bytes())
            .unwrap();
        let module_name = fixer_name.replace(['-', '.'], "_");
        dest.write_all(format!("mod {} {{\n", module_name).as_bytes())
            .unwrap();

        for test in tests {
            let test = test.unwrap();
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

    // rebuild if build.rs or tests directory changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=tests");
    println!("cargo:rerun-if-changed=renamed-tags.json");
    println!("cargo:rerun-if-changed=/usr/share/lintian/data/obsolete-sites/obsolete-sites");
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
    code.push_str(
        "pub fn get_renamed_tags() -> std::collections::HashMap<&'static str, &'static str> {\n",
    );
    code.push_str("    let mut map = std::collections::HashMap::new();\n");

    for (old_tag, new_tag) in renames {
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
