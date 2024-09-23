use breezyshim::branch::Branch;
use breezyshim::error::Error;
use breezyshim::graph::{Error as GraphError, Graph};
use breezyshim::revisionid::RevisionId;
use breezyshim::tree::{Tree, WorkingTree};
use debian_changelog::{ChangeLog, Entry as ChangeLogEntry};

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
        Err(Error::NoSuchFile(_)) => return false,
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
            Err(Error::NoSuchFile(_)) => {
                log::debug!("No changelog found");
            }
            Err(e) => {
                panic!("Unexpected error reading changelog: {:?}", e);
            }
        }
    }
    if let Some(ref cl) = cl {
        if debian_changelog::is_unreleased_inaugural(cl) {
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
    let mut it = graph.iter_lefthand_ancestry(revid, None);
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
                Err(Error::NoSuchFile(_p)) => {}
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
    let changes = cb.change_lines().collect::<Vec<_>>();
    debian_changelog::changes::all_sha_prefixed(
        changes
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
    use std::path::Path;
    fn make_changelog(entries: Vec<String>) -> String {
        format!(
            r###"lintian-brush (0.1) UNRELEASED; urgency=medium

{}
 -- Jelmer Vernooij <jelmer@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
"###,
            entries
                .iter()
                .map(|x| format!("  * {}\n", x))
                .collect::<Vec<_>>()
                .concat()
        )
    }

    #[test]
    fn test_no_gbp_conf() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: true,
                explanation: "Assuming changelog needs to be updated, since it is always changed together with other files in the tree.".to_string(),
            }),
            guess_update_changelog(&tree, Path::new("debian"), None),
        );
    }

    #[test]
    fn test_custom_path() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: true,
                explanation: "Assuming changelog needs to be updated, since it is always changed together with other files in the tree.".to_string(),
            }),
            guess_update_changelog(&tree, Path::new("debian"), None),
        );
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: true,
                explanation: "assuming changelog needs to be updated since gbp dch only supports a debian directory in the root of the repository".to_string(),
            }),
            guess_update_changelog(&tree, Path::new(""), None),
        );
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: true,
                explanation: "assuming changelog needs to be updated since gbp dch only supports a debian directory in the root of the repository".to_string(),
            }),
            guess_update_changelog(&tree, Path::new("lala/debian"), None),
        );
    }

    #[test]
    fn test_gbp_conf_dch() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/gbp.conf"),
            r#"[dch]
pristine-tar = False
"#,
        )
        .unwrap();
        tree.add(&[Path::new("debian"), Path::new("debian/gbp.conf")])
            .unwrap();
        assert_eq!(Some(ChangelogBehaviour{
                update_changelog: false,
                explanation: "Assuming changelog does not need to be updated, since there is a [dch] section in gbp.conf.".to_string(),
        }),
            guess_update_changelog(&tree, Path::new("debian"), None)
        );
    }

    #[test]
    fn test_changelog_sha_prefixed() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            r#"blah (0.20.1) unstable; urgency=medium

  [ Somebody ]
  * [ebb7c31] do a thing
  * [629746a] do another thing that actually requires us to wrap lines
    and then

  [ Somebody Else ]
  * [b02b435] do another thing

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
"#,
        )
        .unwrap();
        tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
            .unwrap();
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: false,
                explanation: "Assuming changelog does not need to be updated, since all entries in last changelog entry are prefixed by git shas.".to_string(),
            }),
            guess_update_changelog(&tree, Path::new("debian"), None)
        );
    }

    #[test]
    fn test_empty() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        assert_eq!(
            Some(ChangelogBehaviour{
                update_changelog: true,
                explanation: "Assuming changelog needs to be updated, since it is always changed together with other files in the tree.".to_string(),
            }),
            guess_update_changelog(&tree, Path::new("debian"), None)
        );
    }

    #[test]
    fn test_update_with_change() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::write(td.path().join("upstream"), b"upstream").unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            make_changelog(vec!["initial release".to_string()]),
        )
        .unwrap();
        std::fs::write(td.path().join("debian/control"), b"initial").unwrap();
        tree.add(&[
            Path::new("upstream"),
            Path::new("debian"),
            Path::new("debian/changelog"),
            Path::new("debian/control"),
        ])
        .unwrap();
        tree.build_commit()
            .message("initial release")
            .commit()
            .unwrap();
        let mut changelog_entries = vec!["initial release".to_string()];
        for i in 0..20 {
            std::fs::write(td.path().join("upstream"), format!("upstream {}", i)).unwrap();
            changelog_entries.push(format!("next entry {}", i));
            std::fs::write(
                td.path().join("debian/changelog"),
                make_changelog(changelog_entries.clone()),
            )
            .unwrap();
            std::fs::write(td.path().join("debian/control"), format!("next {}", i)).unwrap();
            tree.build_commit().message("Next").commit().unwrap();
        }
        assert_eq!(Some(ChangelogBehaviour {
            update_changelog: true,
            explanation: "Assuming changelog needs to be updated, since it is always changed together with other files in the tree.".to_string(),
        }), guess_update_changelog(&tree, Path::new("debian"), None));
    }

    #[test]
    fn test_changelog_updated_separately() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            make_changelog(vec!["initial release".to_string()]),
        )
        .unwrap();
        std::fs::write(td.path().join("debian/control"), b"initial").unwrap();
        tree.add(&[
            Path::new("debian"),
            Path::new("debian/changelog"),
            Path::new("debian/control"),
        ])
        .unwrap();
        tree.build_commit()
            .message("initial release")
            .commit()
            .unwrap();
        let mut changelog_entries = vec!["initial release".to_string()];
        for i in 0..20 {
            changelog_entries.push(format!("next entry {}", i));
            std::fs::write(
                td.path().join("debian/control"),
                format!("next {}", i).as_bytes(),
            )
            .unwrap();
            tree.build_commit().message("Next").commit().unwrap();
        }
        std::fs::write(
            td.path().join("debian/changelog"),
            make_changelog(changelog_entries.clone()),
        )
        .unwrap();
        tree.build_commit().message("Next").commit().unwrap();
        changelog_entries.push("final entry".to_string());
        std::fs::write(td.path().join("debian/control"), b"more").unwrap();
        tree.build_commit().message("Next").commit().unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            make_changelog(changelog_entries),
        )
        .unwrap();
        tree.build_commit().message("Next").commit().unwrap();
        assert_eq!(Some(ChangelogBehaviour{
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since changelog entries are usually updated in separate commits.".to_string(),
        }), guess_update_changelog(&tree, Path::new("debian"), None));
    }

    #[test]
    fn test_has_dch_in_messages() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        tree.build_commit()
            .message("Git-Dch: ignore\n")
            .allow_pointless(true)
            .commit()
            .unwrap();

        assert_eq!(Some(ChangelogBehaviour{
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since there are Gbp-Dch stanzas in commit messages".to_string(),
        }), guess_update_changelog(&tree, Path::new("debian"), None));
    }

    #[test]
    fn test_inaugural_unreleased() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            r#"blah (0.20.1) UNRELEASED; urgency=medium

  * Initial release. Closes: #123123

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
"#,
        )
        .unwrap();
        tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
            .unwrap();
        assert_eq!(Some(ChangelogBehaviour{
            update_changelog: false,
            explanation: "assuming changelog does not need to be updated since it is the inaugural unreleased entry".to_string(),
        }), guess_update_changelog(&tree, Path::new("debian"), None));
    }

    #[test]
    fn test_last_entry_warns_generated() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            r#"blah (0.20.1) UNRELEASED; urgency=medium

  * WIP (generated at release time: please do not add entries below).

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100

blah (0.20.1) unstable; urgency=medium

  * Initial release. Closes: #123123

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
"#,
        )
        .unwrap();
        tree.add(&[&Path::new("debian"), &Path::new("debian/changelog")])
            .unwrap();
        assert_eq!(
            Some(ChangelogBehaviour {
                update_changelog: false,
                explanation: "last changelog entry warns changelog is generated at release time"
                    .to_string()
            }),
            guess_update_changelog(&tree, Path::new("debian"), None)
        );
    }

    #[test]
    fn test_never_unreleased() {
        let td = tempfile::tempdir().unwrap();
        let tree = create_standalone_workingtree(td.path(), &ControlDirFormat::default()).unwrap();
        std::fs::create_dir(td.path().join("debian")).unwrap();
        std::fs::write(td.path().join("debian/control"), b"foo").unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            r#"blah (0.20.1) unstable; urgency=medium

  * Initial release. Closes: #123123

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
"#,
        )
        .unwrap();

        tree.add(&[
            (Path::new("debian")),
            (Path::new("debian/control")),
            (Path::new("debian/changelog")),
        ])
        .unwrap();
        tree.build_commit().message("rev1").commit().unwrap();
        std::fs::write(td.path().join("debian/control"), b"bar").unwrap();
        tree.build_commit().message("rev2").commit().unwrap();
        std::fs::write(td.path().join("debian/control"), b"bla").unwrap();
        tree.build_commit().message("rev2").commit().unwrap();
        std::fs::write(
            td.path().join("debian/changelog"),
            r#"blah (0.21.1) unstable; urgency=medium

  * Next release.

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100

blah (0.20.1) unstable; urgency=medium

  * Initial release. Closes: #123123

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
"#,
        )
        .unwrap();
        tree.build_commit().message("rev2").commit().unwrap();
        assert_eq!(Some(ChangelogBehaviour{
            update_changelog: false,
            explanation: "Assuming changelog does not need to be updated, since it never uses UNRELEASED entries".to_string()
        }), guess_update_changelog(&tree, Path::new("debian"), None));
    }
}
