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

from breezy.errors import NotBranchError

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
