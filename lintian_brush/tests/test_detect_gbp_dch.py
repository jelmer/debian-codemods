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

"""Tests for lintian_brush.detect_gbp_dch."""

from ..detect_gbp_dch import (
    guess_update_changelog,
    )

from breezy.tests import (
    TestCaseWithTransport,
    )


def make_changelog(entries):
    return ("""\
lintian-brush (0.1) UNRELEASED; urgency=medium

""" + ''.join(["  * %s\n" % entry for entry in entries]) + """

 -- Jelmer Vernooij <jelmer@debian.org>  Sat, 13 Oct 2018 11:21:39 +0100
""").encode('utf-8')


class GuessUpdateChangelogTests(TestCaseWithTransport):

    def test_no_gbp_conf(self):
        tree = self.make_branch_and_tree('.')
        self.assertTrue(guess_update_changelog(tree))

    def test_gbp_conf_dch(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/gbp.conf', """\
[dch]
""")])
        tree.add(['debian', 'debian/gbp.conf'])
        self.assertEqual(
            (False, 'Assuming changelog does not need to be updated, since '
             'there is a [dch] section in gbp.conf.'),
            guess_update_changelog(tree))

    def test_changelog_sha_prefixed(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
blah (0.20.1) unstable; urgency=medium

  [ Somebody ]
  * [ebb7c31] do a thing
  * [629746a] do another thing that actually requires us to wrap lines
    and then

  [ Somebody Else ]
  * [b02b435] do another thing

 -- Joe User <joe@example.com>  Tue, 19 Nov 2019 15:29:47 +0100
""")])
        tree.add(['debian', 'debian/changelog'])
        self.assertEqual(
            (False, 'Assuming changelog does not need to be updated, '
             'since all entries in last changelog entry are prefixed '
             'by git shas.'),
            guess_update_changelog(tree))

    def test_empty(self):
        tree = self.make_branch_and_tree('.')
        self.assertEqual(
            (True,
             'Assuming changelog needs to be updated, '
             'since it is always changed together '
             'with other files in the tree.'),
            guess_update_changelog(tree))

    def test_update_with_change(self):
        builder = self.make_branch_builder('.')
        builder.start_series()
        builder.build_snapshot(None, [
            ('add', ('', None, 'directory', '')),
            ('add', ('upstream', None, 'file', b'upstream')),
            ('add', ('debian/', None, 'directory', '')),
            ('add', ('debian/changelog', None, 'file',
                     make_changelog(['initial release']))),
            ('add', ('debian/control', None, 'file', b'initial'))],
            message='Initial\n')
        changelog_entries = ['initial release']
        for i in range(20):
            builder.build_snapshot(None, [
                ('modify', ('upstream', b'upstream %d' % i))],
                message='Upstream')
            changelog_entries.append('next entry %d' % i)
            builder.build_snapshot(None, [
                ('modify', ('debian/changelog',
                 make_changelog(changelog_entries))),
                ('modify', ('debian/control', b'next %d' % i))],
                message='Next')
        builder.finish_series()
        branch = builder.get_branch()
        tree = branch.controldir.create_workingtree()
        self.assertEqual(
            (True, 'Assuming changelog needs to be updated, '
             'since it is always changed together '
             'with other files in the tree.'), guess_update_changelog(tree))

    def test_changelog_updated_separately(self):
        builder = self.make_branch_builder('.')
        builder.start_series()
        builder.build_snapshot(None, [
            ('add', ('', None, 'directory', '')),
            ('add', ('debian/', None, 'directory', '')),
            ('add', ('debian/changelog', None, 'file',
             make_changelog(['initial release']))),
            ('add', ('debian/control', None, 'file', b'initial'))],
            message='Initial\n')
        changelog_entries = ['initial release']
        for i in range(20):
            changelog_entries.append('next entry %d' % i)
            builder.build_snapshot(None, [
                ('modify', ('debian/control', b'next %d' % i))],
                message='Next\n')
        builder.build_snapshot(None, [
            ('modify', ('debian/changelog',
             make_changelog(changelog_entries)))])
        changelog_entries.append('final entry')
        builder.build_snapshot(None, [
            ('modify', ('debian/control', b'more'))],
            message='Next\n')
        builder.build_snapshot(None, [
            ('modify', ('debian/changelog',
             make_changelog(changelog_entries)))])
        builder.finish_series()
        branch = builder.get_branch()
        tree = branch.controldir.create_workingtree()
        self.assertEqual(
            (False,
             'Assuming changelog does not need to be updated, '
             'since changelog entries are usually updated in '
             'separate commits.'), guess_update_changelog(tree))

    def test_has_dch_in_messages(self):
        builder = self.make_branch_builder('.')
        builder.build_snapshot(None, [
            ('add', ('', None, 'directory', ''))],
            message='Git-Dch: ignore\n')
        branch = builder.get_branch()
        tree = branch.controldir.create_workingtree()
        self.assertEqual(
            (False, 'Assuming changelog does not need to be updated, '
             'since there are Gbp-Dch stanzas in commit messages'),
            guess_update_changelog(tree))
