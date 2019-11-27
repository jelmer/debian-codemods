#!/usr/bin/python3
# Copyright (C) 2019 Jelmer Vernooij
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

"""Handling of quilt patches."""

import contextlib
import os
import tempfile

from breezy import osutils
from breezy.commit import filter_excluded
import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.errors import NotBranchError, NoSuchFile
from breezy.patches import parse_patches
from breezy.patch import write_to_cmd

from debian.changelog import Changelog


def find_patch_base(tree):
    """Find the base revision to apply patches to.

    Args:
      tree: Tree to find the patch base for
    Returns:
      A revision string
    """
    with tree.get_file('debian/changelog') as f:
        cl = Changelog(f, max_blocks=1)
        package = cl.package
        upstream_version = cl.version.upstream_version
    possible_tags = [
        'upstream-%s' % upstream_version,
        'upstream/%s' % upstream_version,
        '%s' % upstream_version,
        'v%s' % upstream_version,
        '%s-%s' % (package, upstream_version),
        ]
    tags = tree.branch.tags.get_tag_dict()
    for possible_tag in possible_tags:
        if possible_tag in tags:
            return tags[possible_tag]
    # TODO(jelmer): Do something clever, like look for the last merge?
    return None


def find_patches_branch(tree):
    """Find the branch that is used to track patches.

    Args:
      tree: Tree for which to find patches branch
    Returns:
      A `Branch` instance
    """
    if tree.branch.name is None:
        return None
    branch_name = 'patch-queue/%s' % tree.branch.name
    try:
        return tree.branch.controldir.open_branch(branch_name)
    except NotBranchError:
        pass
    if tree.branch.name == 'master':
        branch_name = 'patched'
    else:
        branch_name = 'patched-%s' % tree.branch.name
    try:
        return tree.branch.controldir.open_branch(branch_name)
    except NotBranchError:
        pass
    return None


# Copied from lp:~jelmer/brz/transform-patches
def apply_patches(tt, patches, prefix=1):
    """Apply patches to a TreeTransform.

    :param tt: TreeTransform instance
    :param patches: List of patches
    """
    def strip_prefix(p):
        return '/'.join(p.split('/')[prefix:])

    from breezy.bzr.generate_ids import gen_file_id
    # TODO(jelmer): Extract and set mode
    for patch in patches:
        if patch.oldname == b'/dev/null':
            trans_id = None
            orig_contents = b''
        else:
            oldname = strip_prefix(patch.oldname.decode())
            trans_id = tt.trans_id_tree_path(oldname)
            orig_contents = tt._tree.get_file_text(oldname)
            tt.delete_contents(trans_id)

        if patch.newname != b'/dev/null':
            new_contents = iter_patched_from_hunks(
                orig_contents.splitlines(True), patch.hunks)
            if trans_id is None:
                newname = strip_prefix(patch.newname.decode())
                parts = os.path.split(newname)
                trans_id = tt.root
                for part in parts[1:-1]:
                    trans_id = tt.new_directory(part, trans_id)
                tt.new_file(
                    parts[-1], trans_id, new_contents,
                    file_id=gen_file_id(newname))
            else:
                tt.create_file(new_contents, trans_id)


class AppliedPatches(object):
    """Context that provides access to a tree with patches applied.
    """

    def __init__(self, tree, patches, prefix=1):
        self.tree = tree
        self.patches = patches
        self.prefix = prefix

    def __enter__(self):
        if self.patches:
            from breezy.transform import TransformPreview
            self._tt = TransformPreview(self.tree)
            apply_patches(self._tt, self.patches, prefix=self.prefix)
            return self._tt.get_preview_tree()
        else:
            self._tt = None
            return self.tree

    def __exit__(self, exc_type, exc_value, exc_tb):
        if self._tt:
            self._tt.finalize()
        return False


def read_quilt_patches(tree, directory='debian/patches'):
    """Read patch contents from quilt directory.

    Args:
      tree: Tree to read
      directory: Patch directory
    Returns:
      list of Patch objects
    """
    series_path = osutils.pathjoin(directory, 'series')
    try:
        series_lines = tree.get_file_lines(series_path)
    except NoSuchFile:
        return []
    for line in series_lines:
        line = line.strip().split(b'#')[0].strip()
        if not line:
            continue
        patchname = line.decode()
        with tree.get_file(osutils.pathjoin(directory, patchname)) as f:
            for patch in parse_patches(f, allow_dirty=True, keep_dirty=False):
                yield patch


class PatchApplicationBaseNotFound(Exception):
    """Unable to find base for patch application."""


@contextlib.contextmanager
def upstream_with_applied_patches(tree, patches):
    """Create a copy of the upstream tree with applied patches.

    Args:
      tree: Tree with applied patches
    """
    patches_branch = find_patches_branch(tree)
    if patches_branch is not None:
        # TODO(jelmer): Make sure it's actually rebased on current upstream
        yield patches_branch.basis_tree()
        return

    upstream_revision = find_patch_base(tree)
    if upstream_revision is None:
        raise PatchApplicationBaseNotFound(tree)
    upstream_tree = tree.branch.repository.revision_tree(upstream_revision)

    with AppliedPatches(upstream_tree, patches) as tree:
        yield tree


def tree_non_patches_changes(tree):
    """Check if a Debian tree has changes vs upstream tree.

    Args:
      tree: Tree to check
    Returns:
        list of TreeDelta objects
    """
    directory = 'debian/patches'
    patches = list(read_quilt_patches(tree, directory))

    # TODO(jelmer): What if patches are already applied on tree?
    with upstream_with_applied_patches(tree, patches) \
            as upstream_patches_tree, \
            AppliedPatches(tree, patches) as patches_tree:
        for change in filter_excluded(
                patches_tree.iter_changes(upstream_patches_tree),
                exclude=['debian']):
            try:
                path = change.path[1]
            except AttributeError:  # breezy < 3.1
                path = change[1][1]
            if path == '':
                continue
            yield change


# Copied from lp:~jelmer/brz/patch-api
def iter_patched_from_hunks(orig_lines, hunks):
    """Iterate through a series of lines with a patch applied.
    This handles a single file, and does exact, not fuzzy patching.

    :param orig_lines: The unpatched lines.
    :param hunks: An iterable of Hunk instances.

    This is different from breezy.patches in that it invokes the patch
    command.
    """
    with tempfile.NamedTemporaryFile() as f:
        f.writelines(orig_lines)
        f.flush()
        # TODO(jelmer): Stream patch contents to command, rather than
        # serializing the entire patch upfront.
        serialized = b''.join([hunk.as_bytes() for hunk in hunks])
        args = ["patch", "-f", "-s", "--posix", "--binary",
                "-o", "-", f.name, "-r", "-"]
        stdout, stderr, status = write_to_cmd(args, serialized)
    if status == 0:
        return [stdout]
    raise Exception(stderr)
