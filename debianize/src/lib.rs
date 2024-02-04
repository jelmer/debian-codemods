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

impl ToString for BugKind {
    fn to_string(&self) -> String {
        match self {
            BugKind::RFP => "RFP".to_string(),
            BugKind::ITP => "ITP".to_string(),
        }
    }
}

#[cfg(feature = "pyo3")]
impl pyo3::FromPyObject<'_> for BugKind {
    fn extract(ob: &pyo3::PyAny) -> pyo3::PyResult<Self> {
        let s: String = ob.extract()?;
        s.parse()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
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
