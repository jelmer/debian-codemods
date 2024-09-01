use crate::release_info::{suite_to_distribution, Vendor};
use breezyshim::commit::CommitReporter;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::{Kind, MutableTree, Path, Tree, WorkingTree};
use breezyshim::RevisionId;
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

pub fn changelog_changes(
    tree: &dyn Tree,
    basis_tree: &dyn Tree,
    cl_path: &Path,
) -> Result<Option<Vec<String>>, BrzError> {
    let mut changes = vec![];
    for change in tree.iter_changes(basis_tree, Some(&[cl_path]), None, None)? {
        let change = change?;
        let paths = change.path;
        let changed_content = change.changed_content;
        let versioned = change.versioned;
        let kind = change.kind;
        // Content not changed
        if !changed_content {
            return Ok(None);
        }
        // Not versioned in new tree
        if !versioned.1.unwrap_or(false) {
            return Ok(None);
        }
        // Not a file in one tree
        if kind.0 != Some(Kind::File) || kind.1 != Some(Kind::File) {
            return Ok(None);
        }

        let old_text = basis_tree.get_file_lines(&paths.0.unwrap())?;
        let new_text = tree.get_file_lines(&paths.1.unwrap())?;
        changes.extend(new_changelog_entries(&old_text, &new_text));
    }
    Ok(Some(changes))
}

/// Strip a changelog message like debcommit does.
///
/// Takes a list of changes from a changelog entry and applies a transformation
/// so the message is well formatted for a commit message.
///
/// # Arguments
/// * `changes` - a list of lines from the changelog entry
///
/// # Returns
/// another list of lines with blank lines stripped from the start
/// and the spaces the start of the lines split if there is only one
/// logical entry.
pub fn strip_changelog_message(changes: &[&str]) -> Vec<String> {
    if changes.is_empty() {
        return vec![];
    }
    let mut changes = changes.to_vec();
    while changes.last() == Some(&"") {
        changes.pop();
    }
    while changes.first() == Some(&"") {
        changes.remove(0);
    }

    let changes = changes
        .into_iter()
        .map(|l| lazy_regex::regex_replace!(r"  |\t", l, |_| ""))
        .collect::<Vec<_>>();

    let leader_re = lazy_regex::regex!(r"^[ \t]*[*+-] ");
    let leader_changes = changes
        .iter()
        .filter(|line| leader_re.is_match(line))
        .collect::<Vec<_>>();

    if leader_changes.len() == 1 {
        changes
            .iter()
            .map(|line| leader_re.replace(line, "").trim_start().to_string())
            .collect()
    } else {
        changes.into_iter().map(|l| l.to_string()).collect()
    }
}

pub fn changelog_commit_message(
    tree: &dyn Tree,
    basis_tree: &dyn Tree,
    path: &Path,
) -> Result<String, BrzError> {
    let changes = changelog_changes(tree, basis_tree, path)?;
    let changes = changes.unwrap_or_default();

    Ok(strip_changelog_message(
        changes
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .concat())
}

/// Create a git commit with message based on the new entries in changelog.
///
/// # Arguments
/// * `tree` - Tree to commit in
/// * `committer` - Optional committer identity
/// * `subpath` - subpath to commit in
/// * `paths` - specifics paths to commit, if any
/// * `reporter` - CommitReporter to use
///
/// # Returns
/// Created revision id
pub fn debcommit(
    tree: &WorkingTree,
    committer: Option<&str>,
    subpath: &Path,
    paths: Option<&[&Path]>,
    reporter: Option<&dyn CommitReporter>,
    message: Option<&str>,
) -> Result<RevisionId, BrzError> {
    let message = message.map_or_else(
        || {
            changelog_commit_message(
                tree,
                &tree.basis_tree().unwrap(),
                &subpath.join("debian/changelog"),
            )
            .unwrap()
        },
        |m| m.to_string(),
    );
    let specific_files = if let Some(paths) = paths {
        Some(paths.iter().map(|p| subpath.join(p)).collect())
    } else if !subpath.to_str().unwrap().is_empty() {
        Some(vec![subpath.to_path_buf()])
    } else {
        None
    };

    let mut builder = tree.build_commit().message(&message);

    if let Some(reporter) = reporter {
        builder = builder.reporter(reporter);
    }

    if let Some(committer) = committer {
        builder = builder.committer(committer);
    }

    if let Some(specific_files) = specific_files {
        builder = builder.specific_files(
            specific_files
                .iter()
                .map(|p| p.as_path())
                .collect::<Vec<_>>()
                .as_slice(),
        );
    }

    builder.commit()
}

pub fn new_changelog_entries(old_text: &[Vec<u8>], new_text: &[Vec<u8>]) -> Vec<String> {
    let mut sm = difflib::sequencematcher::SequenceMatcher::new(old_text, new_text);
    let mut changes = vec![];
    for group in sm.get_grouped_opcodes(0) {
        let (j1, j2) = (group[0].second_start, group.last().unwrap().second_end);
        for line in new_text[j1..j2].iter() {
            if line.starts_with(b"  ") {
                // Debian Policy Manual states that debian/changelog must be UTF-8
                changes.push(String::from_utf8_lossy(line).to_string());
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    mod strip_changelog_message {
        use super::*;

        #[test]
        fn test_empty() {
            assert_eq!(strip_changelog_message(&[]), Vec::<String>::new());
        }

        #[test]
        fn test_empty_changes() {
            assert_eq!(strip_changelog_message(&[""]), Vec::<String>::new());
        }

        #[test]
        fn test_removes_leading_whitespace() {
            assert_eq!(
                strip_changelog_message(&["foo", "  bar", "\tbaz", "   bang"]),
                vec!["foo", "bar", "baz", " bang"],
            );
        }

        #[test]
        fn test_removes_star_if_one() {
            assert_eq!(strip_changelog_message(&["  * foo"]), ["foo"]);
            assert_eq!(strip_changelog_message(&["\t* foo"]), ["foo"]);
            assert_eq!(strip_changelog_message(&["  + foo"]), ["foo"]);
            assert_eq!(strip_changelog_message(&["  - foo"]), ["foo"]);
            assert_eq!(strip_changelog_message(&["  *  foo"]), ["foo"]);
            assert_eq!(
                strip_changelog_message(&["  *  foo", "     bar"]),
                ["foo", "bar"]
            );
        }

        #[test]
        fn test_leaves_start_if_multiple() {
            assert_eq!(
                strip_changelog_message(&["  * foo", "  * bar"]),
                ["* foo", "* bar"]
            );
            assert_eq!(
                strip_changelog_message(&["  * foo", "  + bar"]),
                ["* foo", "+ bar"]
            );
            assert_eq!(
                strip_changelog_message(&["  * foo", "  bar", "  * baz"]),
                ["* foo", "bar", "* baz"],
            );
        }
    }
}
