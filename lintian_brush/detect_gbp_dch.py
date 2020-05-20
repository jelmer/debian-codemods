#!/usr/bin/python
# Copyright (C) 2020 Jelmer Vernooij
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

"""Detect gbp dch policy."""


from debian.changelog import Changelog
import itertools
import re
from typing import Optional

from breezy import osutils, version_info as breezy_version
from breezy.trace import note
from breezy.branch import Branch
from breezy.errors import NoSuchFile
from breezy.tree import Tree
from breezy.workingtree import WorkingTree


# Number of revisions to search back
DEFAULT_BACKLOG = 50


def gbp_conf_has_dch_section(tree: WorkingTree,
                             path: str = '') -> Optional[bool]:
    try:
        gbp_conf_path = osutils.pathjoin(path, 'debian/gbp.conf')
        gbp_conf_text = tree.get_file_text(gbp_conf_path)
    except NoSuchFile:
        return False
    try:
        import configparser
    except ImportError:
        return None
    else:
        parser = configparser.ConfigParser(defaults={}, strict=False)
        parser.read_string(
            gbp_conf_text.decode('utf-8', errors='replace'), gbp_conf_path)
        return parser.has_section('dch')


def all_sha_prefixed(cl: Changelog) -> bool:
    sha_prefixed = 0
    for change in cl.changes():
        if not change.startswith('  * '):
            continue
        if re.match(r'  \* \[[0-9a-f]{7}\] ', change):
            sha_prefixed += 1
        else:
            return False
    return (sha_prefixed > 0)


_changelog_policy_noted = False


def guess_update_changelog(tree: WorkingTree,
                           path: str = '',
                           cl: Optional[Changelog] = None) -> bool:
    """Guess whether the changelog should be updated.

    Args:
      tree: Tree to edit
      path: Path to packaging in tree
    Returns:
      best guess at whether we should update changelog (bool)
    """
    ret = _guess_update_changelog_from_tree(tree, path, cl)
    if ret is not None:
        return ret
    ret = _guess_update_changelog_from_branch(tree.branch, path)
    if ret is not None:
        return ret
    # Assume true
    return None


def _guess_update_changelog_from_tree(
        tree: Tree, path: str = '',
        cl: Optional[Changelog] = None) -> Optional[bool]:
    global _changelog_policy_noted
    if gbp_conf_has_dch_section(tree, path):
        if not _changelog_policy_noted:
            note('Assuming changelog does not need to be updated, '
                 'since there is a [dch] section in gbp.conf.')
            _changelog_policy_noted = True
        return False

    # TODO(jelmes): Do something more clever here, perhaps looking at history
    # of the changelog file?
    changelog_path = osutils.pathjoin(path, 'debian/changelog')
    if cl is None:
        try:
            with tree.get_file(changelog_path) as f:
                cl = Changelog(f, max_blocks=1)
        except NoSuchFile:
            cl = None
    if cl:
        if all_sha_prefixed(cl[0]):
            return False

    return None


def _changelog_stats(branch, history, subpath):
    mixed = 0
    changelog_only = 0
    other_only = 0
    dch_references = 0
    with branch.lock_read():
        graph = branch.repository.get_graph()
        revids = list(itertools.islice(
            graph.iter_lefthand_ancestry(branch.last_revision()), history))
        revs = []
        for revid, rev in branch.repository.iter_revisions(revids):
            if rev is None:
                # Ghost
                continue
            if 'Git-Dch: ' in rev.message:
                dch_references += 1
            revs.append(rev)
        for delta in branch.repository.get_deltas_for_revisions(revs):
            if breezy_version >= (3, 1):
                filenames = set(
                    [a.path[1] for a in delta.added] +
                    [r.path[0] for r in delta.removed] +
                    [r.path[0] for r in delta.renamed] +
                    [r.path[1] for r in delta.renamed] +
                    [m.path[0] for m in delta.modified])
            else:
                filenames = set([a[0] for a in delta.added] +
                                [r[0] for r in delta.removed] +
                                [r[1] for r in delta.renamed] +
                                [m[0] for m in delta.modified])
            if not set([
                    f for f in filenames
                    if f.startswith(osutils.pathjoin(subpath, 'debian/'))]):
                continue
            if osutils.pathjoin(subpath, 'debian/changelog') in filenames:
                if len(filenames) > 1:
                    mixed += 1
                else:
                    changelog_only += 1
            else:
                other_only += 1
    return (changelog_only, other_only, mixed, dch_references)


def _guess_update_changelog_from_branch(
        branch: Branch, subpath: str = '',
        history: int = DEFAULT_BACKLOG) -> Optional[bool]:
    """Guess whether the changelog should be updated manually.

    Args:
      branch: A branch object
      history: Number of revisions back to analyze
    Returns:
      boolean indicating whether changelog should be updated
    """
    # Two indications this branch may be doing changelog entries at
    # release time:
    # - "Git-Dch: " is used in the commit messages
    # - The vast majority of lines in changelog get added in
    #   commits that only touch the changelog
    (changelog_only, other_only, mixed, dch_references) = _changelog_stats(
            branch, history, subpath)
    if dch_references:
        return False
    if changelog_only == 0:
        return True
    if mixed == 0 and mixed == 0:
        # changelog is *always* updated in a separate commit.
        return False
    if changelog_only > mixed:
        # Is this a reasonable threshold?
        return False
    # Assume yes
    return True
