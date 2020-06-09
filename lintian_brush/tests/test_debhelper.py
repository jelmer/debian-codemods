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

"""Tests for lintian_brush.debhelper."""

from breezy.tests import (
    TestCase,
    )

from ..debhelper import (
    lowest_non_deprecated_compat_level,
    highest_stable_compat_level,
    ensure_minimum_debhelper_version,
    )


class LowestNonDeprecatedCompatLevel(TestCase):

    def test_is_valid(self):
        self.assertIsInstance(lowest_non_deprecated_compat_level(), int)


class HighestStableCompatLevelTests(TestCase):

    def test_is_valid(self):
        self.assertIsInstance(highest_stable_compat_level(), int)


class EnsureMinumumDebhelperVersionTests(TestCase):

    def test_already(self):
        d = {
            'Build-Depends': 'debhelper (>= 10)',
            }
        self.assertFalse(ensure_minimum_debhelper_version(d, '10'))
        self.assertEqual(d, {'Build-Depends': 'debhelper (>= 10)'})
        self.assertFalse(ensure_minimum_debhelper_version(d, '9'))
        self.assertEqual(d, {'Build-Depends': 'debhelper (>= 10)'})

    def test_already_compat(self):
        d = {
            'Build-Depends': 'debhelper-compat (= 10)',
            }
        self.assertFalse(ensure_minimum_debhelper_version(d, '10'))
        self.assertEqual(d, {'Build-Depends': 'debhelper-compat (= 10)'})
        self.assertFalse(ensure_minimum_debhelper_version(d, '9'))
        self.assertEqual(d, {'Build-Depends': 'debhelper-compat (= 10)'})

    def test_bump(self):
        d = {
            'Build-Depends': 'debhelper (>= 10)',
            }
        self.assertTrue(ensure_minimum_debhelper_version(d, '11'))
        self.assertEqual(d, {'Build-Depends': 'debhelper (>= 11)'})

    def test_bump_compat(self):
        d = {
            'Build-Depends': 'debhelper-compat (= 10)',
            }
        self.assertTrue(ensure_minimum_debhelper_version(d, '11'))
        self.assertEqual(d, {
            'Build-Depends': 'debhelper-compat (= 10), debhelper (>= 11)'})
        self.assertTrue(ensure_minimum_debhelper_version(d, '11.1'))
        self.assertEqual(d, {
            'Build-Depends': 'debhelper-compat (= 10), debhelper (>= 11.1)'})

    def test_not_set(self):
        d = {}
        self.assertTrue(ensure_minimum_debhelper_version(d, '10'))
        self.assertEqual(d, {'Build-Depends': 'debhelper (>= 10)'})

    def test_in_indep(self):
        d = {'Build-Depends-Indep': 'debhelper (>= 9)'}
        self.assertRaises(Exception, ensure_minimum_debhelper_version, d, '10')
