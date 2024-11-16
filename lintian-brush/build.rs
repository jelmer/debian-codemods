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
    let fixers_dir = Path::new(&manifest_dir).join("fixers");
    let test_dir = Path::new(&manifest_dir).join("tests");

    // Load the fixers/index.desc file as yaml
    let f = fs::File::open(fixers_dir.join("index.desc")).unwrap();

    #[derive(serde::Deserialize)]
    struct Fixer {
        script: String,
        #[serde(rename = "lintian-tags")]
        lintian_tags: Option<Vec<String>>,
    }

    let fixers: Vec<Fixer> = serde_yaml::from_reader(f).unwrap();

    for fixer in fixers {
        let script = fixer.script.clone();

        let script_path = fixers_dir.join(&script).to_str().unwrap().to_string();

        let fixer_name = fixer.script.trim_end_matches(".py").trim_end_matches(".rs");

        // Discover the tests for this fixer
        let tests = match fs::read_dir(test_dir.join(fixer_name)) {
            Ok(tests) => tests,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("Failed to read test directory: {}", e),
        };

        dest.write_all(format!("mod {} {{\n", fixer_name.replace('-', "_")).as_bytes())
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

            let tags = fixer.lintian_tags.clone().unwrap_or_default();

            let test = quote! {
                #[test]
                fn #fn_name() {
                    crate::fixer_tests::run_fixer_testcase(#fixer_name, std::path::Path::new(#script_path), #test_name, std::path::Path::new(#test_path), &[#(#tags),*]);
                }
            };

            // Write the test to the output file
            dest.write_all(test.to_string().as_bytes()).unwrap();
        }

        dest.write_all("}\n".as_bytes()).unwrap();
    }

    // rebuild if build.rs or fixers/index.desc changes
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fixers/index.desc");
}
