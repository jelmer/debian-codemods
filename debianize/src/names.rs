pub fn source_name_from_directory_name(path: &std::path::Path) -> String {
    let d = path.file_name().unwrap().to_str().unwrap();
    if d.contains('-') {
        let mut parts = d.split('-').collect::<Vec<_>>();
        let c = parts.last().unwrap().chars().next().unwrap();
        if c.is_ascii_digit() {
            parts.pop();
            return parts.join("-");
        }
    }
    d.to_string()
}

pub fn go_import_path_from_repo(repo_url: &url::Url) -> String {
    repo_url.host_str().unwrap().to_string()
        + repo_url
            .path()
            .trim_end_matches('/')
            .trim_end_matches(".git")
}

pub fn perl_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name.strip_prefix("lib").unwrap_or(upstream_name);
    format!(
        "lib{}-perl",
        upstream_name
            .replace("::", "-")
            .replace('_', "")
            .to_lowercase()
    )
}

pub fn python_source_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name
        .strip_prefix("python-")
        .unwrap_or(upstream_name);
    format!("python-{}", upstream_name.replace('_', "-").to_lowercase())
}

pub fn python_binary_package_name(upstream_name: &str) -> String {
    let upstream_name = upstream_name
        .strip_prefix("python-")
        .unwrap_or(upstream_name);
    format!("python3-{}", upstream_name.replace('_', "-").to_lowercase())
}

pub fn debian_to_upstream_version(version: &str) -> &str {
    // Drop debian-specific modifiers from an upstream version string.
    version.split("+dfsg").next().unwrap_or_default()
}

pub fn upstream_name_to_debian_source_name(mut upstream_name: &str) -> Option<String> {
    if let Some((_, _, abbrev)) = lazy_regex::regex_captures!(r"^(.{10,})\((.*)\)", upstream_name) {
        upstream_name = abbrev;
    }

    // Remove "GNU " prefix
    if upstream_name.starts_with("GNU ") {
        upstream_name = &upstream_name["GNU ".len()..];
    }

    // Convert to lowercase and replace characters
    Some(upstream_name.to_lowercase().replace(['_', ' ', '/'], "-"))
}

pub fn upstream_package_to_debian_source_name(family: &str, name: &str) -> Option<String> {
    match family {
        "rust" => Some(format!("rust-{}", name.to_lowercase())),
        "perl" => Some(format!(
            "lib{}-perl",
            name.to_lowercase().replace("::", "-")
        )),
        "node" => Some(format!("node-{}", name.to_lowercase())),
        _ => upstream_name_to_debian_source_name(name),
    }
}

pub fn upstream_package_to_debian_binary_name(family: &str, name: &str) -> String {
    match family {
        "rust" => format!("rust-{}", name.to_lowercase()),
        "perl" => format!("lib{}-perl", name.to_lowercase().replace("::", "-")),
        "node" => format!("node-{}", name.to_lowercase()),
        _ => name.to_lowercase().replace('_', "-"),
    }
}

pub fn go_base_name(package: &str) -> String {
    let (mut hostname, path) = package.split_once('/').unwrap();
    if hostname == "github.com" {
        hostname = "github";
    }
    if hostname == "gopkg.in" {
        hostname = "gopkg";
    }
    let path = path.trim_end_matches('/').replace(['/', '_'], "-");
    let path = path.strip_suffix(".git").unwrap_or(&path);
    [hostname, path].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gnu() {
        assert_eq!(Some("lala"), upstream_name_to_debian_source_name("GNU Lala").as_deref());
    }

    #[test]
    fn test_abbrev() {
        assert_eq!(
            Some("mun"),
            upstream_name_to_debian_source_name("Made Up Name (MUN)").as_deref()
        );
    }

    #[test]
    fn test_source_name_from_directory_name() {
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo")),
            "foo"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar")),
            "foo-bar"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar-1")),
            "foo-bar"
        );
        assert_eq!(
            source_name_from_directory_name(std::path::Path::new("foo-bar-1.0")),
            "foo-bar"
        );
    }

    #[test]
    fn test_go_import_path_from_repo() {
        assert_eq!(
            go_import_path_from_repo(&url::Url::parse("https://github.com/foo/bar.git").unwrap()),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn test_perl_package_name() {
        assert_eq!(perl_package_name("Foo::Bar"), "libfoo-bar-perl");
        assert_eq!(perl_package_name("Foo::Bar::Baz"), "libfoo-bar-baz-perl");
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux"),
            "libfoo-bar-baz-qux-perl"
        );
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux::Quux"),
            "libfoo-bar-baz-qux-quux-perl"
        );
        assert_eq!(
            perl_package_name("Foo::Bar::Baz::Qux::Quux::Corge"),
            "libfoo-bar-baz-qux-quux-corge-perl"
        );
    }

    #[test]
    fn test_python_source_package_name() {
        assert_eq!(python_source_package_name("foo"), "python-foo");
        assert_eq!(
            python_source_package_name("python-foo_bar"),
            "python-foo-bar"
        );
        assert_eq!(python_source_package_name("foo_bar"), "python-foo-bar");
        assert_eq!(
            python_source_package_name("foo_bar_baz"),
            "python-foo-bar-baz"
        );
    }

    #[test]
    fn test_python_binary_package_name() {
        assert_eq!(python_binary_package_name("foo"), "python3-foo");
        assert_eq!(
            python_binary_package_name("python-foo_bar"),
            "python3-foo-bar"
        );
        assert_eq!(python_binary_package_name("foo_bar"), "python3-foo-bar");
        assert_eq!(
            python_binary_package_name("foo_bar_baz"),
            "python3-foo-bar-baz"
        );
    }
}
