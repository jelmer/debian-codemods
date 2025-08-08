#[test]
fn test_ognibuild_buildsystem_names() {
    use std::fs;
    use tempfile::TempDir;

    // Test what build system names ognibuild returns
    let test_cases = vec![
        ("CMakeLists.txt", "cmake\nproject(test)\n"),
        ("configure.ac", "AC_INIT([test], [1.0])\n"),
        ("configure.in", "AC_INIT([test], [1.0])\n"),
        ("Makefile", "all:\n\techo 'test'\n"),
        (
            "setup.py",
            "from setuptools import setup\nsetup(name='test')\n",
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        ),
        ("go.mod", "module test\n\ngo 1.20\n"),
        (
            "package.json",
            "{\"name\": \"test\", \"version\": \"1.0.0\"}\n",
        ),
        ("pom.xml", "<project></project>\n"),
        ("dist.ini", "name = Test\n"),
        ("META.json", "{\"name\": \"Test\"}\n"),
        ("Build.PL", "use Module::Build;\n"),
        ("Makefile.PL", "use ExtUtils::MakeMaker;\n"),
        ("DESCRIPTION", "Package: test\n"),
        ("DESCRIPTION.in", "Package: test\n"),
    ];

    println!("\nOgnibuild build system detection:");
    for (filename, content) in test_cases {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join(filename);
        fs::write(&file_path, content).unwrap();

        let buildsystems = ognibuild::buildsystem::detect_buildsystems(temp_dir.path());
        let buildsystem = buildsystems.into_iter().next();

        if let Some(bs) = buildsystem {
            println!("{:20} -> {}", filename, bs.name());
        } else {
            println!("{:20} -> No build system detected", filename);
        }
    }
}
