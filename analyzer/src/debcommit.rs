use crate::release_info::{suite_to_distribution, Vendor};
use breezyshim::tree::{Tree, WorkingTree};
use debian_changelog::ChangeLog;

#[derive(Debug)]
pub enum Error {
    UnreleasedChanges(std::path::PathBuf),
    ChangelogError(debian_changelog::Error),
    BrzError(breezyshim::error::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::UnreleasedChanges(path) => write!(f, "Unreleased changes in {}", path.display()),
            Error::ChangelogError(e) => write!(f, "{}", e),
            Error::BrzError(e) => write!(f, "{}", e),
        }
    }
}

impl From<breezyshim::error::Error> for Error {
    fn from(e: breezyshim::error::Error) -> Self {
        Error::BrzError(e)
    }
}

impl From<debian_changelog::Error> for Error {
    fn from(e: debian_changelog::Error) -> Self {
        Error::ChangelogError(e)
    }
}

impl std::error::Error for Error {}

pub fn debcommit_release(
    tree: &WorkingTree,
    committer: Option<&str>,
    subpath: Option<&std::path::Path>,
    message: Option<&str>,
    vendor: Option<Vendor>,
) -> Result<String, Error> {
    let subpath = subpath.unwrap_or_else(|| std::path::Path::new(""));
    let cl_path = subpath.join("debian/changelog");
    let (message, vendor) = if let (Some(message), Some(vendor)) = (message, vendor) {
        (message.to_string(), vendor)
    } else {
        let f = tree.get_file(&cl_path)?;
        let cl = ChangeLog::read(f)?;
        let entry = cl.entries().next().unwrap();
        let message = if let Some(message) = message {
            message.to_string()
        } else {
            format!(
                "releasing package {} version {}",
                entry.package().unwrap(),
                entry.version().unwrap()
            )
        };
        let vendor = vendor.unwrap_or_else(|| {
            suite_to_distribution(
                entry
                    .distributions()
                    .as_ref()
                    .and_then(|d| d.first())
                    .unwrap(),
            )
            .unwrap()
        });
        (message, vendor)
    };
    let tag_name = if let Ok(tag_name) = breezyshim::debian::tree_debian_tag_name(
        tree,
        tree.branch().as_ref(),
        Some(subpath),
        Some(vendor.to_string()),
    ) {
        tag_name
    } else {
        return Err(Error::UnreleasedChanges(cl_path));
    };

    let mut builder = tree.build_commit().message(&message);

    if let Some(committer) = committer {
        builder = builder.committer(committer);
    }

    let revid = builder.commit()?;
    tree.branch().tags().unwrap().set_tag(&tag_name, &revid)?;
    Ok(tag_name)
}
