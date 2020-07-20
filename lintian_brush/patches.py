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
from email.message import Message
from io import BytesIO
import os

from breezy.diff import show_diff_trees
from breezy import osutils
from breezy.commit import filter_excluded
import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.errors import NotBranchError, NoSuchFile
from breezy.patches import parse_patches, apply_patches

from debian.changelog import Changelog

from . import reset_tree


DEFAULT_DEBIAN_PATCHES_DIR = 'debian/patches'


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


# TODO(jelmer): Use debmutate version
def read_quilt_series(f):
    for line in f:
        if line.startswith(b'#'):
            quoted = True
            line = line.split(b'#')[1].strip()
        else:
            quoted = False
        args = line.decode().split()
        if not args:
            continue
        patch = args[0]
        if not patch:
            continue
        options = args[1:]
        yield patch, quoted, options


def read_quilt_patches(tree, directory=DEFAULT_DEBIAN_PATCHES_DIR):
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
    for patchname, quoted, options in read_quilt_series(series_lines):
        if quoted:
            continue
        # TODO(jelmer): Pass on options?
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


def rules_find_patches_directory(makefile):
    """Find the patches directory set in debian/rules.

    Args:
        makefile: Makefile to scan
    Returns:
        path to patches directory, or None if none was found in debian/rules
    """
    try:
        val = makefile.get_variable(b'QUILT_PATCH_DIR')
    except KeyError:
        return None
    else:
        return val.decode()


def find_patches_directory(path):
    """Find the name of the patches directory, if any.

    Args:
      path: Root to package
    Returns:
      relative path to patches directory, or None if none exists
    """
    from .rules import Makefile
    directory = None
    try:
        mf = Makefile.from_path(os.path.join(path, 'debian/rules'))
    except FileNotFoundError:
        pass
    else:
        rules_directory = rules_find_patches_directory(mf)
        if rules_directory is not None:
            directory = rules_directory
    if directory is None and os.path.exists(
            os.path.join(path, DEFAULT_DEBIAN_PATCHES_DIR)):
        directory = DEFAULT_DEBIAN_PATCHES_DIR
    return directory


def tree_patches_directory(tree):
    """Find the name of the patches directory.

    This will always return a path, even if the patches
    directory does not yet exist.

    Args:
      tree: Tree to check
    Returns:
      path to patches directory, or what it should be
    """
    directory = find_patches_directory(tree.abspath('.'))
    if directory is None:
        return DEFAULT_DEBIAN_PATCHES_DIR
    return directory


def tree_non_patches_changes(tree, patches_directory):
    """Check if a Debian tree has changes vs upstream tree.

    Args:
      tree: Tree to check
      patches_directory: Name of patches directory
    Returns:
        list of TreeDelta objects
    """
    if patches_directory is None:
        return
    patches = list(read_quilt_patches(tree, patches_directory))

    # TODO(jelmer): What if patches are already applied on tree?
    with upstream_with_applied_patches(tree, patches) \
            as upstream_patches_tree, \
            AppliedPatches(tree, patches) as patches_tree:
        for change in filter_excluded(
                patches_tree.iter_changes(upstream_patches_tree),
                exclude=['debian']):
            path = change.path[1]
            if path == '':
                continue
            yield change


# TODO(jelmer): Use debmutate version
def find_common_patch_suffix(names, default='.patch'):
    """Find the common prefix to use for patches.

    Args:
      names: List of filenames in debian/patches/
      default: Default suffix if no default can be found
    Returns:
      a suffix
    """
    suffix_count = {}
    for name in names:
        if name in ('series', '00list'):
            continue
        if name.startswith('README'):
            continue
        suffix = os.path.splitext(name)[1]
        if suffix not in suffix_count:
            suffix_count[suffix] = 0
        suffix_count[suffix] += 1
    if not suffix_count:
        return default
    return max(suffix_count.items(), key=lambda v: v[1])[0]


def add_patch(tree, patches_directory, name, contents, header=None):
    """Add a new patch.

    Args:
      tree: Tree to edit
      patches_directory: Name of patches directory
      name: Patch name without suffix
      contents: Diff
      header: RFC822 to read
    Returns:
      Name of the patch that was written (including suffix)
    """
    if not tree.has_filename(patches_directory):
        tree.mkdir(patches_directory)
    patch_suffix = find_common_patch_suffix(os.listdir(patches_directory))
    patchname = name + patch_suffix
    path = os.path.join(patches_directory, patchname)
    if tree.has_filename(path):
        raise FileExistsError(path)
    with open(tree.abspath(path), 'wb') as f:
        if header is not None:
            f.write(header.as_string().encode('utf-8'))
            f.write(b'\n')
        f.write(contents)

    # TODO(jelmer): Write to patches branch if applicable

    # TODO(jelmer): Use debmutate SeriesEditor
    series_path = os.path.join(patches_directory, 'series')
    with open(tree.abspath(series_path, 'a')) as f:
        f.write('%s\n' % patchname)

    return patchname


def move_upstream_changes_to_patch(
        local_tree, subpath, patch_name, description,
        dirty_tracker=None):
    """Move upstream changes to patch.

    Args:
      local_tree: local tree
      subpath: subpath
      patch_name: Suggested patch name
      description: Description
      dirty_tracker: Dirty tracker
    """
    diff = BytesIO()
    basis_tree = local_tree.basis_tree()
    show_diff_trees(basis_tree, local_tree, diff)
    reset_tree(local_tree, dirty_tracker, subpath)
    header = Message()
    lines = description.splitlines()
    header['Description'] = lines[0].rstrip('\n')
    header.set_payload(''.join([line + '\n' for line in lines[1:]]).lstrip())
    patches_directory = tree_patches_directory(local_tree)
    patchname = add_patch(
        local_tree, patches_directory, patch_name, diff.getvalue(), header)
    specific_files = [
        os.path.join(subpath, patches_directory),
        os.path.join(subpath, patches_directory, 'series'),
        os.path.join(subpath, patches_directory, patchname)]
    local_tree.add(specific_files)
    return specific_files, patchname
