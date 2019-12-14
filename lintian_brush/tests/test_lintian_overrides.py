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

"""Tests for lintian_brush.lintian_overrides."""

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush.lintian_overrides import (
    Override,
    overrides_paths,
    override_exists,
    parse_override,
    serialize_override,
    update_overrides_file,
    _get_overrides,
    )


class OverridesPathTests(TestCaseWithTransport):

    def test_no_overrides_paths(self):
        self.assertEqual([], list(overrides_paths()))

    def test_overrides_paths(self):
        self.build_tree(
            ['debian/', 'debian/source/', 'debian/source/lintian-overrides'])
        self.assertEqual(
            ['debian/source/lintian-overrides'],
            list(overrides_paths()))


class UpdateOverridesFileTests(TestCaseWithTransport):

    def test_no_changes(self):
        CONTENT = """\
# An architecture wildcard would look like:
foo [any-i386] binary: another-tag optional-extra
"""
        self.build_tree_contents([('overrides', CONTENT)])

        def cb(override):
            return override

        self.assertFalse(update_overrides_file(cb=cb, path='overrides'))
        self.assertFileEqual(CONTENT, 'overrides')

    def test_change_set_archlist(self):
        self.build_tree_contents([('overrides', """\
# An architecture wildcard would look like:
foo binary: another-tag optional-extra
""")])

        def cb(override):
            return Override(
                tag=override.tag, package=override.package,
                type=override.type, info=override.info,
                archlist='any-i386')

        self.assertTrue(update_overrides_file(cb=cb, path='overrides'))
        self.assertFileEqual("""\
# An architecture wildcard would look like:
foo [any-i386] binary: another-tag optional-extra
""", 'overrides')


class ParseOverrideTests(TestCase):

    def test_tag_only(self):
        self.assertEqual(
            Override(tag='sometag'),
            parse_override('sometag\n'))

    def test_origin(self):
        self.assertEqual(
            Override(tag='sometag', package='mypkg'),
            parse_override('mypkg: sometag\n'))

    def test_archlist(self):
        self.assertEqual(
            Override(tag='sometag', archlist='i386 amd64'),
            parse_override('[i386 amd64]: sometag\n'))


class SerializeOverrideTests(TestCase):

    def test_tag_only(self):
        self.assertEqual(
            serialize_override(Override(tag='sometag')),
            'sometag\n')

    def test_origin(self):
        self.assertEqual(
            serialize_override(Override(tag='sometag', package='mypkg')),
            'mypkg: sometag\n')

    def test_archlist(self):
        self.assertEqual(
            serialize_override(Override(tag='sometag', archlist='i386')),
            '[i386]: sometag\n')


class OverrideExistsTests(TestCaseWithTransport):

    def test_override_exists(self):
        self.build_tree_contents([
            ('debian/', ),
            ('debian/source/', ),
            ('debian/source/lintian-overrides', """\
blah source: patch-file-exists-but info
""")])
        self.assertEqual([
            Override(package='blah', archlist=None, type='source',
                     tag='patch-file-exists-but', info='info')],
            list(_get_overrides()))
        self.assertTrue(override_exists('patch-file-exists-but', info='info'))
        self.assertFalse(override_exists('patch-file-exists-but', info='no'))
        self.assertTrue(override_exists(
            'patch-file-exists-but', info='info', package='source'))
