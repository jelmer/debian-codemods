use super::*;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/fixer_tests.rs"));

fn run_fixer_testcase(
    _fixer_name: &str,
    script_path: &Path,
    test_name: &str,
    path: &Path,
    tags: &[&str],
) {
    #[cfg(feature = "python")]
    {
        pyo3::Python::with_gil(|py| {
            use pyo3::prelude::*;
            let sys = py.import_bound("sys").unwrap();
            let path = sys.getattr("path").unwrap();
            let mut path: Vec<String> = path.extract().unwrap();
            let extra_path =
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string() + "/../py")
                    .canonicalize()
                    .unwrap();
            if !path.contains(&extra_path.to_string_lossy().to_string()) {
                path.insert(0, extra_path.to_string_lossy().to_string());
                sys.setattr("path", path).unwrap();
            }
        });
    }
    let td = tempfile::tempdir().unwrap();

    let indir = path.join("in");
    let outdir = path.join("out");

    let testdir = td.path().join("testdir");
    std::fs::create_dir(&testdir).unwrap();

    // recursively copy indir to td/in
    let mut options = fs_extra::dir::CopyOptions::new();
    options.copy_inside = true;
    options.content_only = true;
    fs_extra::dir::copy(indir, &testdir, &options).unwrap();

    let xfail_path = path.join("xfail");
    match std::fs::read_to_string(&xfail_path) {
        Ok(s) => {
            eprintln!(
                "Skipping test {} because it is expected to fail: {}",
                test_name, s
            );
            return;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("Error reading {}: {}", xfail_path.display(), e),
    }

    let mut env = HashMap::new();
    for name in ["PATH"] {
        if let Some(value) = std::env::var_os(name) {
            env.insert(name.to_string(), value.to_string_lossy().to_string());
        }
    }

    let cl_path = testdir.join("debian/changelog");
    let current_version = match std::fs::File::open(&cl_path) {
        Ok(f) => {
            let cl = ChangeLog::read(f).unwrap();
            let first_entry = cl.iter().next().unwrap();
            let version = first_entry.version().unwrap();
            if first_entry.distributions().as_deref().unwrap() == vec!["UNRELEASED"] {
                version
            } else {
                let mut version = version;
                version.increment_debian();
                version
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "1.0-1".parse().unwrap(),
        Err(e) => panic!("Error reading {}: {}", cl_path.display(), e),
    };

    env.insert("CURRENT_VERSION".to_owned(), current_version.to_string());
    env.insert("NET_ACCESS".to_owned(), "disallow".to_string());
    env.insert("MINIMUM_CERTAINTY".to_owned(), "possible".to_string());
    env.insert("PYTHONPATH".to_owned(), {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../py")
            .canonicalize()
            .unwrap();
        let mut path = pyo3::Python::with_gil(|py| {
            use pyo3::prelude::*;
            py.import_bound("sys")
                .unwrap()
                .getattr("path")
                .unwrap()
                .extract::<Vec<String>>()
        })
        .unwrap();
        path.insert(0, p.to_string_lossy().to_string());
        path.join(":")
    });

    let env_path = path.join("env");
    match std::fs::File::open(&env_path) {
        Ok(f) => {
            use std::io::BufRead;
            let br = std::io::BufReader::new(f);
            for line in br.lines() {
                let line = line.unwrap();
                let (name, value) = line.split_once('=').unwrap();
                env.insert(name.to_string(), value.to_string());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("Error reading {}: {}", env_path.display(), e),
    }

    let output = std::process::Command::new(script_path)
        .current_dir(testdir.clone())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .output()
        .unwrap();

    if output.status.code() != Some(0) {
        eprintln!("Output:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("Error:\n{}", String::from_utf8_lossy(&output.stderr));
        panic!("Test {} failed with exit code {}", test_name, output.status);
    }

    let diff_output = std::process::Command::new("diff")
        .arg("--no-dereference")
        .arg("-x")
        .arg("*~")
        .arg("-ur")
        .arg({
            if outdir.is_symlink() {
                path.join(std::fs::read_link(&outdir).unwrap())
            } else {
                outdir.clone()
            }
        })
        .arg(testdir)
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap();

    if diff_output.status.code() != Some(0) && diff_output.status.code() != Some(1) {
        panic!("Unexpected diff status: {}", diff_output.status);
    }

    if !diff_output.stdout.is_empty() {
        let diff = String::from_utf8_lossy(&diff_output.stdout);
        eprintln!("Diff:\n{}", diff);
        panic!("Test {} failed", test_name);
    }

    let check_message =
        if !outdir.is_symlink() || outdir.read_link().unwrap() != PathBuf::from("in") {
            let output = String::from_utf8_lossy(&output.stdout);
            let result = parse_script_fixer_output(&output).unwrap();

            let got_tags: HashSet<&str> = result.fixed_lintian_tags().into_iter().collect();
            let expected_tags: HashSet<&str> = tags.iter().copied().collect();

            // the got_tags should be a subset of the expected tags
            if !got_tags.is_subset(&expected_tags) {
                eprintln!("Expected tags: {:?}", expected_tags);
                eprintln!("Got tags: {:?}", got_tags);
                panic!("Test {} failed", test_name);
            }
            true
        } else {
            false
        };

    let message_path = path.join("message");
    match std::fs::read_to_string(&message_path) {
        Ok(expected_message) => {
            let got_message = String::from_utf8_lossy(&output.stdout);
            if got_message != expected_message {
                eprintln!("Expected message:\n{:?}", expected_message);
                eprintln!("Got message:\n{:?}", got_message);
                panic!("Test {} failed", test_name);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if check_message {
                panic!("No message file found for test {}", test_name);
            }
        }
        Err(e) => panic!("Error reading {}: {}", message_path.display(), e),
    }
}
