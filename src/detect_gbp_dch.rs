use breezyshim::branch::Branch;
use breezyshim::graph::{Error as GraphError, Graph};
use breezyshim::revisionid::RevisionId;
use breezyshim::tree::{Error as TreeError, Tree, WorkingTree};
use debian_changelog::{ChangeLog, Entry as ChangeLogEntry};
use lazy_regex::regex;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub struct ChangelogBehaviour {
    #[serde(rename = "update")]
    pub update_changelog: bool,
    pub explanation: String,
}

impl From<ChangelogBehaviour> for (bool, String) {
    fn from(b: ChangelogBehaviour) -> Self {
        (b.update_changelog, b.explanation)
    }
}

impl From<&ChangelogBehaviour> for (bool, String) {
    fn from(b: &ChangelogBehaviour) -> Self {
        (b.update_changelog, b.explanation.clone())
    }
}

// Number of revisions to search back
const DEFAULT_BACKLOG: usize = 50;

// TODO(jelmer): Check that what's added in the changelog is actually based on
// what was in the commit messages?

pub fn gbp_conf_has_dch_section(tree: &dyn Tree, debian_path: &std::path::Path) -> bool {
    let gbp_conf_path = debian_path.join("gbp.conf");
    let gbp_conf_text = match tree.get_file_text(gbp_conf_path.as_path()) {
        Ok(text) => text,
        Err(TreeError::NoSuchFile(_)) => return false,
        Err(e) => panic!("Unexpected error reading gbp.conf: {:?}", e),
    };

    let mut parser = configparser::ini::Ini::new();
    parser
        .read(String::from_utf8_lossy(gbp_conf_text.as_slice()).to_string())
        .unwrap();
    parser.sections().contains(&"dch".to_string())
}

/// Guess whether the changelog should be updated.
///
/// # Arguments
/// * `tree` - Tree to edit
/// * `debian_path` - Path to packaging in tree
///
/// # Returns
/// * `None` if it is not possible to guess
/// * `True` if the changelog should be updated
/// * `False` if the changelog should not be updated
pub fn guess_update_changelog(
    tree: &WorkingTree,
    debian_path: &std::path::Path,
    mut cl: Option<ChangeLog>,
) -> Option<ChangelogBehaviour> {
    if debian_path != std::path::Path::new("debian") {
        return Some(ChangelogBehaviour{
            update_changelog: true,
            explanation: "assuming changelog needs to be updated since gbp dch only supports a debian directory in the root of the repository".to_string(),
        });
    }
    let changelog_path = debian_path.join("changelog");
    if cl.is_none() {
        match tree.get_file(changelog_path.as_path()) {
            Ok(f) => {
                cl = Some(ChangeLog::read(f).unwrap());
            }
            Err(TreeError::NoSuchFile(_)) => {
                log::debug!("No changelog found");
            }
            Err(e) => {
                panic!("Unexpected error reading changelog: {:?}", e);
            }
        }
    }
    if let Some(ref cl) = cl {
        if is_unreleased_inaugural(cl) {
            return Some(ChangelogBehaviour {
                update_changelog: false,
                explanation: "assuming changelog does not need to be updated since it is the inaugural unreleased entry".to_string()
            });
        }
        if let Some(first_entry) = cl.entries().next() {
            for line in first_entry.change_lines() {
                if line.contains("generated at release time") {
                    return Some(ChangelogBehaviour {
                        update_changelog: false,
                        explanation:
                            "last changelog entry warns changelog is generated at release time"
                                .to_string(),
                    });
                }
            }
        }
    }
    if let Some(ret) = guess_update_changelog_from_tree(tree, debian_path, cl) {
        Some(ret)
    } else {
        guess_update_changelog_from_branch(tree.branch().as_ref(), debian_path, None)
    }
}

pub fn guess_update_changelog_from_tree(
    tree: &dyn Tree,
    debian_path: &std::path::Path,
    cl: Option<ChangeLog>,
) -> Option<ChangelogBehaviour> {
    if gbp_conf_has_dch_section(tree, debian_path) {
        return Some(ChangelogBehaviour {
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since there is a [dch] section in gbp.conf.".to_string()
        });
    }

    // TODO(jelmes): Do something more clever here, perhaps looking at history of the changelog file?
    if let Some(cl) = cl {
        if let Some(entry) = cl.entries().next() {
            if all_sha_prefixed(&entry) {
                return Some(ChangelogBehaviour {
                    update_changelog: false,
                    explanation: "Assuming changelog does not need to be updated, since all entries in last changelog entry are prefixed by git shas.".to_string()
                });
            }
        }
    }

    None
}

pub fn greedy_revisions(
    graph: &Graph,
    revid: &RevisionId,
    length: usize,
) -> (Vec<RevisionId>, bool) {
    let mut ret = vec![];
    let mut it = graph.iter_lefthand_ancestry(revid);
    while ret.len() < length {
        ret.push(match it.next() {
            None => break,
            Some(Ok(rev)) => rev,
            Some(Err(GraphError::RevisionNotPresent(_))) => {
                if !ret.is_empty() {
                    ret.pop();
                }
                // Shallow history
                return (ret, true);
            }
        });
    }
    (ret, false)
}

#[derive(Debug, Default)]
struct ChangelogStats {
    mixed: usize,
    changelog_only: usize,
    other_only: usize,
    dch_references: usize,
    unreleased_references: usize,
}

fn changelog_stats(
    branch: &dyn Branch,
    history: usize,
    debian_path: &std::path::Path,
) -> ChangelogStats {
    let mut ret = ChangelogStats::default();
    let branch_lock = branch.lock_read();
    let graph = branch.repository().get_graph();
    let (revids, _truncated) = greedy_revisions(&graph, &branch.last_revision(), history);
    let mut revs = vec![];
    for (_revid, rev) in branch.repository().iter_revisions(revids) {
        if rev.is_none() {
            // Ghost
            continue;
        }
        let rev = rev.unwrap();
        if rev.message.contains("Git-Dch: ") || rev.message.contains("Gbp-Dch: ") {
            ret.dch_references += 1;
        }
        revs.push(rev);
    }
    for (rev, delta) in revs.iter().zip(
        branch
            .repository()
            .get_revision_deltas(revs.as_slice(), None),
    ) {
        let mut filenames = vec![];
        for a in delta.added {
            if let Some(p) = a.path.1 {
                filenames.push(p.clone());
            }
        }
        for r in delta.removed {
            if let Some(p) = r.path.0 {
                filenames.push(p.clone());
            }
        }
        for r in delta.renamed {
            if let Some(p) = r.path.0 {
                filenames.push(p.clone());
            }
            if let Some(p) = r.path.1 {
                filenames.push(p.clone());
            }
        }
        for m in delta.modified {
            if let Some(p) = m.path.0 {
                filenames.push(p.clone());
            }
        }
        if !filenames.iter().any(|f| f.starts_with(debian_path)) {
            continue;
        }
        let cl_path = debian_path.join("changelog");
        if filenames.contains(&cl_path) {
            let revtree = branch.repository().revision_tree(&rev.revision_id).unwrap();
            match revtree.get_file_lines(cl_path.as_path()) {
                Err(TreeError::NoSuchFile(_p)) => {}
                Err(e) => {
                    panic!("Error reading changelog: {}", e);
                }
                Ok(cl_lines) => {
                    if String::from_utf8_lossy(cl_lines[0].as_slice()).contains("UNRELEASED") {
                        ret.unreleased_references += 1;
                    }
                }
            }
            if filenames.len() > 1 {
                ret.mixed += 1;
            } else {
                ret.changelog_only += 1;
            }
        } else {
            ret.other_only += 1;
        }
    }
    std::mem::drop(branch_lock);
    ret
}

/// Guess whether the changelog should be updated manually.
///
/// # Arguments
///
///  * `branch` - A branch object
///  * `debian_path` - Path to the debian directory
///  * `history` - Number of revisions back to analyze
///
/// # Returns
///
///   boolean indicating whether changelog should be updated
pub fn guess_update_changelog_from_branch(
    branch: &dyn Branch,
    debian_path: &std::path::Path,
    history: Option<usize>,
) -> Option<ChangelogBehaviour> {
    let history = history.unwrap_or(DEFAULT_BACKLOG);
    // Two indications this branch may be doing changelog entries at
    // release time:
    // - "Git-Dch: " or "Gbp-Dch: " is used in the commit messages
    // - The vast majority of lines in changelog get added in
    //   commits that only touch the changelog
    let stats = changelog_stats(branch, history, debian_path);
    log::debug!("Branch history analysis: changelog_only: {}, other_only: {}, mixed: {}, dch_references: {}, unreleased_references: {}",
                  stats.changelog_only, stats.other_only, stats.mixed, stats.dch_references,
                  stats.unreleased_references);
    if stats.dch_references > 0 {
        return Some(ChangelogBehaviour {
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since there are Gbp-Dch stanzas in commit messages".to_string()
        });
    }
    if stats.changelog_only == 0 {
        return Some(ChangelogBehaviour {
            update_changelog: true,
            explanation: "Assuming changelog needs to be updated, since it is always changed together with other files in the tree.".to_string()
        });
    }
    if stats.unreleased_references == 0 {
        return Some(ChangelogBehaviour {
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since it never uses UNRELEASED entries".to_string()
        });
    }
    if stats.mixed == 0 && stats.changelog_only > 0 && stats.other_only > 0 {
        // changelog is *always* updated in a separate commit.
        return Some(ChangelogBehaviour {
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since changelog entries are always updated in separate commits.".to_string()
        });
    }
    // Is this a reasonable threshold?
    if stats.changelog_only > stats.mixed && stats.other_only > stats.mixed {
        return Some(ChangelogBehaviour{
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since changelog entries are usually updated in separate commits.".to_string()
        });
    }
    None
}

/// This is generally done by gbp-dch(1).
///
/// # Arguments
///
/// * `cl` - Changelog entry
pub fn all_sha_prefixed(cb: &ChangeLogEntry) -> bool {
    let mut sha_prefixed = 0;
    for change in cb.change_lines() {
        if !change.starts_with("* ") {
            continue;
        }
        if regex!(r"\* \[[0-9a-f]{7}\] ").is_match(change.as_str()) {
            sha_prefixed += 1;
        } else {
            return false;
        }
    }

    sha_prefixed > 0
}

/// Check whether this is a traditional inaugural release.
///
/// # Arguments
///
/// * `cl`: A changelog object to inspect
pub fn is_unreleased_inaugural(cl: &ChangeLog) -> bool {
    let mut it = cl.entries();
    let first_entry = match it.next() {
        None => return false,
        Some(e) => e,
    };
    if it.next().is_some() {
        return false;
    }
    if !first_entry
        .distributions()
        .map(|ds| {
            ds.iter()
                .find(|d| distribution_is_unreleased(d.as_str()))
                .is_some()
        })
        .unwrap_or(false)
    {
        return false;
    }
    let actual = first_entry
        .change_lines()
        .filter(|change| !change.trim().is_empty())
        .collect::<Vec<_>>();
    if actual.len() != 1 {
        return false;
    }
    actual[0].starts_with("* Initial release")
}

pub fn distribution_is_unreleased(distribution: &str) -> bool {
    distribution == "UNRELEASED" || distribution.starts_with("UNRELEASED-")
}
