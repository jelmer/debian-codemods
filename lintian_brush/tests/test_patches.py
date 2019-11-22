#!/usr/bin/python
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

"""Tests for lintian_brush.patches."""

from breezy.tests import (
    TestCaseWithTransport,
    )

from ..patches import (
    find_patch_base,
    find_patches_branch,
    )


class FindPatchBaseTests(TestCaseWithTransport):

    def setUp(self):
        super(FindPatchBaseTests, self).setUp()
        self.tree = self.make_branch_and_tree('.')
        self.upstream_revid = self.tree.commit('upstream')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
blah (0.38) unstable; urgency=medium

  * Fix something

 -- Jelmer Vernooij <jelmer@debian.org>  Sat, 19 Oct 2019 15:21:53 +0000
 """)])
        self.tree.add(['debian', 'debian/changelog'])

    def test_none(self):
        self.assertEqual(None, find_patch_base(self.tree))

    def test_upstream_dash(self):
        self.tree.branch.tags.set_tag('upstream-0.38', self.upstream_revid)
        self.assertEqual(
            self.upstream_revid, find_patch_base(self.tree))


class FindPatchBranchTests(TestCaseWithTransport):

    def make_named_branch_and_tree(self, path, name):
        dir = self.make_controldir(path)
        dir.create_repository()
        branch = dir.create_branch(name=name)
        dir.set_branch_reference(branch)
        return dir.create_workingtree()

    def test_none(self):
        tree = self.make_branch_and_tree('.')
        self.assertIs(None, find_patches_branch(tree))

    def test_patch_queue(self):
        master = self.make_named_branch_and_tree('.', name='master')
        master.branch.controldir.create_branch(name='patch-queue/master')

        self.assertEqual(
            'patch-queue/master',
            find_patches_branch(master).name)

    def test_patched_master(self):
        master = self.make_named_branch_and_tree('.', name='master')
        master.branch.controldir.create_branch(name='patched')
        self.assertEqual('patched', find_patches_branch(master).name)

    def test_patched_other(self):
        other = self.make_named_branch_and_tree('.', name='other')
        other.branch.controldir.create_branch(name='patched-other')
        self.assertEqual('patched-other', find_patches_branch(other).name)
