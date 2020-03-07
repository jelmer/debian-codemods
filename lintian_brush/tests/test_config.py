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

"""Tests for lintian_brush.config."""

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from distro_info import DebianDistroInfo

from lintian_brush.config import (
    Config,
    resolve_release_codename,
    )


class ResolveCodeNameTests(TestCase):

    def test_resolve_debian(self):
        self.assertEqual('sid', resolve_release_codename('sid'))
        self.assertEqual('buster', resolve_release_codename('buster'))
        self.assertEqual('sid', resolve_release_codename('unstable'))

    def test_resolve_unknown(self):
        self.assertEqual(None, resolve_release_codename('blah'))

    def test_resolve_ubuntu(self):
        self.assertEqual('trusty', resolve_release_codename('trusty'))


class ConfigReadTests(TestCaseWithTransport):

    def test_compat_release(self):
        debian = DebianDistroInfo()
        self.build_tree_contents([
            ('debian/', ),
            ('debian/lintian-brush.conf', """\
compat-release = testing
""")])
        cfg = Config('debian/lintian-brush.conf')
        self.assertEqual(debian.testing(), cfg.compat_release())

    def test_minimum_certainty(self):
        self.build_tree_contents([
            ('debian/', ),
            ('debian/lintian-brush.conf', """\
minimum-certainty = possible
""")])
        cfg = Config('debian/lintian-brush.conf')
        self.assertEqual('possible', cfg.minimum_certainty())

    def test_update_changelog(self):
        self.build_tree_contents([
            ('debian/', ),
            ('debian/lintian-brush.conf', """\
update-changelog = True
""")])
        cfg = Config('debian/lintian-brush.conf')
        self.assertEqual(True, cfg.update_changelog())

    def test_unknown(self):
        self.build_tree_contents([
            ('debian/', ),
            ('debian/lintian-brush.conf', """\
unknown = dunno
""")])
        with self.assertWarns(Warning):
            Config('debian/lintian-brush.conf')

    def test_missing(self):
        self.assertRaises(FileNotFoundError, Config, 'blah.conf')
