use quote::quote;
use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    let path = manifest_dir.join("../key-package-versions.json");
    println!("cargo:rerun-if-changed={}", path.display());

    let data: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let path = out_dir.join("key_package_versions.rs");

    let mut file = std::fs::File::create(path).unwrap();

    for (key, versions) in data.as_object().unwrap() {
        let key = key.to_string();
        let versions = versions
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_str().unwrap().to_string()))
            .map(|(k, v)| {
                let k = k.as_str();
                let v = v.as_str();
                quote! {
                    map.insert(#k, #v.parse::<debversion::Version>().unwrap());
                }
            })
            .collect::<Vec<_>>();

        let key = quote::format_ident!("{}_versions", key);
        use std::io::Write;

        let code = quote! {
            lazy_static::lazy_static! {
                pub static ref #key: std::collections::HashMap<&'static str, debversion::Version> = {
                    let mut map = std::collections::HashMap::new();
                    #(#versions)*
                    map
                };
            }
        };

        writeln!(file, "{}", code).unwrap();
    }
}
