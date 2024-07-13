use debversion::Version;

pub fn default_debianize_cache_dir() -> std::io::Result<std::path::PathBuf> {
    xdg::BaseDirectories::with_prefix("debianize")?.create_cache_directory("")
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BugKind {
    RFP,
    ITP,
}

impl std::str::FromStr for BugKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RFP" => Ok(BugKind::RFP),
            "ITP" => Ok(BugKind::ITP),
            _ => Err(format!("Unknown bug kind: {}", s)),
        }
    }
}

impl std::fmt::Display for BugKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BugKind::RFP => write!(f, "RFP"),
            BugKind::ITP => write!(f, "ITP"),
        }
    }
}

#[cfg(feature = "pyo3")]
impl pyo3::FromPyObject<'_> for BugKind {
    fn extract_bound(ob: &pyo3::Bound<pyo3::PyAny>) -> pyo3::PyResult<Self> {
        use pyo3::prelude::*;
        let s: String = ob.extract()?;
        s.parse().map_err(pyo3::exceptions::PyValueError::new_err)
    }
}

pub fn write_changelog_template(
    path: &std::path::Path,
    source_name: &str,
    version: &Version,
    author: Option<(String, String)>,
    wnpp_bugs: Option<Vec<(BugKind, u32)>>,
) -> Result<(), std::io::Error> {
    let author = author.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap());
    let closes = if let Some(wnpp_bugs) = wnpp_bugs {
        format!(
            " Closes: {}",
            wnpp_bugs
                .iter()
                .map(|(_k, n)| format!("#{}", n))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        "".to_string()
    };
    let mut cl = debian_changelog::ChangeLog::new();

    cl.new_entry()
        .package(source_name.to_string())
        .version(version.clone())
        .distribution("UNRELEASED".to_string())
        .urgency(debian_changelog::Urgency::Low)
        .change_line(format!("  * Initial release.{}", closes))
        .maintainer(author)
        .finish();

    let buf = cl.to_string();

    std::fs::write(path, buf)?;

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
