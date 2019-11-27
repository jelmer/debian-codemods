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
from breezy.patches import (
    parse_patch,
    Patch,
    )

from ..patches import (
    AppliedPatches,
    find_patch_base,
    find_patches_branch,
    read_quilt_patches,
    tree_non_patches_changes,
    upstream_with_applied_patches,
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


class ReadSeriesFileTests(TestCaseWithTransport):

    def test_no_series_file(self):
        t = self.make_branch_and_tree('.')
        self.assertEqual([], list(read_quilt_patches(t)))

    def test_comments(self):
        t = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/patches/', ),
            ('debian/patches/series',
             "# This file intentionally left blank.\n")])
        self.assertEqual([], list(read_quilt_patches(t)))


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


class AppliedPatchesTests(TestCaseWithTransport):

    def test_apply_simple(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([('a', 'a\n')])
        tree.add('a')
        tree.commit('Add a')
        patch = parse_patch(b"""\
--- a/a
+++ b/a
@@ -1 +1 @@
-a
+b
""".splitlines(True))
        with AppliedPatches(tree, [patch]) as newtree:
            self.assertEqual(b'b\n', newtree.get_file_text('a'))

    def test_apply_delete(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([('a', 'a\n')])
        tree.add('a')
        tree.commit('Add a')
        patch = parse_patch(b"""\
--- a/a
+++ /dev/null
@@ -1 +0,0 @@
-a
""".splitlines(True))
        with AppliedPatches(tree, [patch]) as newtree:
            self.assertFalse(newtree.has_filename('a'))

    def test_apply_add(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([('a', 'a\n')])
        tree.add('a')
        tree.commit('Add a')
        patch = parse_patch(b"""\
--- /dev/null
+++ b/b
@@ -0,0 +1 @@
+b
""".splitlines(True))
        with AppliedPatches(tree, [patch]) as newtree:
            self.assertEqual(b'b\n', newtree.get_file_text('b'))


class ReadQuiltPatchesTests(TestCaseWithTransport):

    def test_read_patches(self):
        patch = """\
--- a/a
+++ b/a
@@ -1,5 +1,5 @@
 line 1
 line 2
-line 3
+new line 3
 line 4
 line 5
"""
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/patches/', ),
            ('debian/patches/series', 'foo\n'),
            ('debian/patches/foo', patch)])
        tree.add(['debian', 'debian/patches', 'debian/patches/series',
                  'debian/patches/foo'])
        tree.commit('add patch')
        patches = list(read_quilt_patches(tree))
        self.assertEqual(1, len(patches))
        self.assertIsInstance(patches[0], Patch)
        self.assertEqual(patch.encode('utf-8'), patches[0].as_bytes())


class UpstreamWithAppliedPatchesTests(TestCaseWithTransport):

    def setUp(self):
        super(UpstreamWithAppliedPatchesTests, self).setUp()
        self.tree = self.make_branch_and_tree('.')
        self.build_tree_contents([('afile', 'some line\n')])
        self.tree.add('afile')
        self.upstream_revid = self.tree.commit('upstream')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
blah (0.38) unstable; urgency=medium

  * Fix something

 -- Jelmer Vernooij <jelmer@debian.org>  Sat, 19 Oct 2019 15:21:53 +0000
 """),
            ('debian/patches/', ),
            ('debian/patches/series', '1.patch\n'),
            ('debian/patches/1.patch', """\
--- a/afile
+++ b/afile
@@ -1 +1 @@
-some line
+another line
--- /dev/null
+++ b/newfile
@@ -0,0 +1 @@
+new line
"""),
            ('unchangedfile', 'unchanged\n')])
        self.tree.add(['debian', 'debian/changelog', 'unchangedfile'])

    def test_upstream_branch(self):
        self.tree.branch.tags.set_tag('upstream/0.38', self.upstream_revid)
        patches = list(read_quilt_patches(self.tree))
        with upstream_with_applied_patches(self.tree, patches) as t:
            self.assertEqual(b'another line\n', t.get_file_text('afile'))
            self.assertEqual(b'new line\n', t.get_file_text('newfile'))
            # TODO(jelmer): PreviewTree appears to be broken
            # self.assertEqual(b'unchanged\n',
            #                  t.get_file_text('unchangedfile'))


class TreePatchesNonPatchesTests(TestCaseWithTransport):

    def setUp(self):
        super(TreePatchesNonPatchesTests, self).setUp()
        self.tree = self.make_branch_and_tree('.')
        self.build_tree_contents([('afile', 'some line\n')])
        self.tree.add('afile')
        self.upstream_revid = self.tree.commit('upstream')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
blah (0.38) unstable; urgency=medium

  * Fix something

 -- Jelmer Vernooij <jelmer@debian.org>  Sat, 19 Oct 2019 15:21:53 +0000
 """),
            ('debian/patches/', ),
            ('debian/patches/series', '1.patch\n'),
            ('debian/patches/1.patch', """\
--- a/afile
+++ b/afile
@@ -1 +1 @@
-some line
+another line
""")])
        self.tree.add(['debian', 'debian/changelog'])

    def test_no_delta(self):
        self.tree.branch.tags.set_tag('upstream/0.38', self.upstream_revid)
        self.assertEqual([], list(tree_non_patches_changes(self.tree)))

    def test_delta(self):
        self.tree.branch.tags.set_tag('upstream/0.38', self.upstream_revid)
        self.build_tree_contents([('anotherfile', 'blah')])
        self.tree.add('anotherfile')
        self.assertEqual(1, len(list(tree_non_patches_changes(self.tree))))
