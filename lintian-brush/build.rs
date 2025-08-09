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
}
