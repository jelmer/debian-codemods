#!/usr/bin/python
# Copyright (C) 2018 Jelmer Vernooij
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

"""Tests for lintian_brush."""

import os

from debian.changelog import Version

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush import (
    Fixer,
    FixerResult,
    NoChanges,
    NotDebianPackage,
    PendingChanges,
    available_lintian_fixers,
    increment_version,
    run_lintian_fixer,
    )

CHANGELOG_FILE = ('debian/changelog', """\
blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #911016)

 -- Blah <example@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""")


class AvailableLintianFixersTest(TestCaseWithTransport):

    def test_find_shell_scripts(self):
        self.build_tree([
            'fixers/',
            'fixers/anotherdir/',
            'fixers/i-fix-a-tag.sh',
            'fixers/i-fix-another-tag.py',
            'fixers/.hidden',
            'fixers/backup-file.sh~',
            'fixers/no-extension'])
        self.build_tree_contents([
            ('fixers/index.desc', """\
Fix-Script: foo.sh
Lintian-Tags: i-fix-a-tag

Fix-Script: bar.sh
Lintian-Tags: i-fix-another-tag, no-extension
""")])
        self.assertEqual(
                [['i-fix-a-tag'], ['i-fix-another-tag', 'no-extension']],
                [fixer.lintian_tags
                 for fixer in available_lintian_fixers('fixers')])


class DummyFixer(Fixer):

    def run(self, basedir, current_version):
        with open(os.path.join(basedir, 'debian/control'), 'a') as f:
            f.write('a new line\n')
        return FixerResult("Fixed some tag.\nExtended description.",
                           ['some-tag'])


class RunLintianFixerTests(TestCaseWithTransport):

    def setUp(self):
        super(RunLintianFixerTests, self).setUp()
        self.tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        self.tree.add(['debian', 'debian/changelog', 'debian/control'])
        self.tree.commit('Initial thingy.')

    def test_pending_changes(self):
        self.build_tree_contents([('debian/changelog', 'blah')])
        with self.tree.lock_write():
            self.assertRaises(
                PendingChanges, run_lintian_fixer,
                self.tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)

    def test_extra(self):
        self.build_tree_contents([('debian/foo', 'blah')])
        with self.tree.lock_write():
            self.assertRaises(
                PendingChanges, run_lintian_fixer,
                self.tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)

    def test_not_debian_tree(self):
        self.tree.remove('debian/changelog')
        os.remove('debian/changelog')
        self.tree.commit("not a debian dir")
        with self.tree.lock_write():
            self.assertRaises(
                NotDebianPackage, run_lintian_fixer,
                self.tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)

    def test_simple_modify(self):
        with self.tree.lock_write():
            fixed_tags, summary = run_lintian_fixer(
                self.tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Fixed some tag.")
        self.assertEqual(['some-tag'], fixed_tags)
        self.assertEqual(2, self.tree.branch.revno())
        self.assertEqual(
                self.tree.get_file_lines('debian/control')[-1],
                b"a new line\n")

    def test_new_file(self):
        class NewFileFixer(Fixer):
            def run(self, basedir, current_version):
                with open(os.path.join(basedir, 'debian/somefile'), 'w') as f:
                    f.write("test")
                return FixerResult("Created new file.", ['some-tag'])
        with self.tree.lock_write():
            fixed_tags, summary = run_lintian_fixer(
                self.tree, NewFileFixer('new-file', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Created new file.")
        self.assertEqual(['some-tag'], fixed_tags)
        rev = self.tree.branch.repository.get_revision(
            self.tree.last_revision())
        self.assertEqual(rev.message, (
            'Created new file.\n'
            'Fixes lintian: some-tag\n'
            'See https://lintian.debian.org/tags/some-tag.html for '
            'more details.\n'))
        self.assertEqual(2, self.tree.branch.revno())
        basis_tree = self.tree.branch.basis_tree()
        with basis_tree.lock_read():
            self.assertEqual(
                    basis_tree.get_file_text('debian/somefile'),
                    b"test")

    def test_rename_file(self):
        class RenameFileFixer(Fixer):
            def run(self, basedir, current_version):
                os.rename(os.path.join(basedir, 'debian/control'),
                          os.path.join(basedir, 'debian/control.blah'))
                return FixerResult("Renamed a file.")
        orig_basis_tree = self.tree.branch.basis_tree()
        with self.tree.lock_write():
            fixed_tags, summary = run_lintian_fixer(
                self.tree, RenameFileFixer('rename', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Renamed a file.")
        self.assertEqual([], fixed_tags)
        self.assertEqual(2, self.tree.branch.revno())
        basis_tree = self.tree.branch.basis_tree()
        with basis_tree.lock_read(), orig_basis_tree.lock_read():
            self.assertFalse(basis_tree.has_filename('debian/control'))
            self.assertTrue(basis_tree.has_filename('debian/control.blah'))
            self.assertNotEqual(orig_basis_tree.get_revision_id(),
                                basis_tree.get_revision_id())
            self.expectFailure(
                'mv --auto appears to be broken',
                self.assertEqual, basis_tree.path2id('debian/control.blah'),
                orig_basis_tree.path2id('debian/control'))

    def test_empty_change(self):
        class EmptyFixer(Fixer):
            def run(self, basedir, current_version):
                return FixerResult("I didn't actually change anything.")
        with self.tree.lock_write():
            self.assertRaises(
                    NoChanges, run_lintian_fixer, self.tree,
                    EmptyFixer('empty', 'some-tag'), update_changelog=False)
        self.assertEqual(1, self.tree.branch.revno())
        with self.tree.lock_read():
            self.assertEqual(
                [], list(self.tree.iter_changes(self.tree.basis_tree())))

    def test_fails(self):
        class FailingFixer(Fixer):
            def run(self, basedir, current_version):
                with open(os.path.join(basedir, 'debian/foo'), 'w') as f:
                    f.write("blah")
                with open(os.path.join(basedir, 'debian/control'), 'a') as f:
                    f.write("foo\n")
                raise Exception("Not successful")
        with self.tree.lock_write():
            self.assertRaises(
                    Exception, run_lintian_fixer, self.tree,
                    FailingFixer('fail', 'some-tag'), update_changelog=False)
        self.assertEqual(1, self.tree.branch.revno())
        with self.tree.lock_read():
            self.assertEqual(
                [], list(self.tree.iter_changes(self.tree.basis_tree())))


class HonorsVcsCommitter(TestCaseWithTransport):

    def make_package_tree(self, format):
        tree = self.make_branch_and_tree('.', format=format)
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control', """\
Source: blah
Vcs-Git: https://example.com/blah
Testsuite: autopkgtest

Binary: blah
Arch: all

"""),
            CHANGELOG_FILE])
        tree.add(['debian', 'debian/changelog', 'debian/control'])
        tree.commit('Initial thingy.')
        return tree

    def make_change(self, tree):
        with tree.lock_write():
            fixed_tags, summary = run_lintian_fixer(
                tree, DummyFixer('dummy', 'some-tag'),
                update_changelog=False)
        self.assertEqual(summary, "Fixed some tag.")
        self.assertEqual(['some-tag'], fixed_tags)
        self.assertEqual(2, tree.branch.revno())
        self.assertEqual(
                tree.get_file_lines('debian/control')[-1],
                b"a new line\n")

    def test_honors_tree_committer_config(self):
        tree = self.make_package_tree('git')
        with open(os.path.join(tree.basedir, '.git/config'), 'w') as f:
            f.write("""\
[user]
  email = jane@example.com
  name = Jane Example

""")

        self.make_change(tree)
        rev = tree.branch.repository.get_revision(tree.branch.last_revision())
        self.assertEqual(rev.committer, 'Jane Example <jane@example.com>')


# TODO(jelmer): run_lintian_fixers


class IncrementVersionTests(TestCase):

    def assertVersion(self, expected, start):
        v = Version(start)
        increment_version(v)
        self.assertEqual(Version(expected), v)

    def test_full(self):
        self.assertVersion('1.0-2', '1.0-1')

    def test_native(self):
        self.assertVersion('1.1', '1.0')
