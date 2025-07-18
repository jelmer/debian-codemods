use quote::quote;
use std::path::PathBuf;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    let path = manifest_dir.join("key-package-versions.json");
    println!("cargo:rerun-if-changed={}", path.display());

    let data: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(&path)
            .unwrap_or_else(|e| panic!("Failed to open {}: {}", path.display(), e)),
    )
    .expect("Failed to parse JSON");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));

    let path = out_dir.join("key_package_versions.rs");

    let mut file = std::fs::File::create(&path)
        .unwrap_or_else(|e| panic!("Failed to create {}: {}", path.display(), e));

    let data_obj = data.as_object().expect("Expected JSON object at root");
    for (key, versions) in data_obj {
        let versions = versions
            .as_object()
            .unwrap_or_else(|| panic!("Expected object for key '{}'", key))
            .iter()
            .map(|(k, v)| {
                let version_str = v.as_str()
                    .unwrap_or_else(|| panic!("Expected string value for key '{}.{}'", key, k));
                quote! {
                    map.insert(#k, #version_str.parse::<debversion::Version>().expect("Invalid version"));
                }
            })
            .collect::<Vec<_>>();

        let key = quote::format_ident!("{}_versions", key);
        use std::io::Write;

        let code = quote! {
            lazy_static::lazy_static! {
                /// A map of key package versions for the package `#key`.
                #[allow(non_upper_case_globals)]
                pub static ref #key: std::collections::HashMap<&'static str, debversion::Version> = {
                    let mut map = std::collections::HashMap::new();
                    #(#versions)*
                    map
                };
            }
        };

        writeln!(file, "{}", code)
            .unwrap_or_else(|e| panic!("Failed to write to output file: {}", e));
    }
}
