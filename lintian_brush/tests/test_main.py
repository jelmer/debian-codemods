#!/usr/bin/python
# Copyright (C) 2022 Jelmer Vernooij
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

"""Tests for lintian_brush.__main__."""

from breezy.tests import (
    TestCase,
)

from lintian_brush import available_lintian_fixers
from lintian_brush.__main__ import (
    DEFAULT_ADDON_FIXERS,
    DEFAULT_VALUE_LINTIAN_BRUSH,
    DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY,
    LINTIAN_BRUSH_TAG_DEFAULT_VALUE,
    LINTIAN_BRUSH_TAG_VALUES,
    calculate_value,
    versions_dict,
)


class VersionsDictTests(TestCase):

    def test_expected_types(self):
        d = versions_dict()
        self.assertIsInstance(d, dict, repr(d))
        for k, v in d.items():
            self.assertIsInstance(k, str, repr(d))
            self.assertIsInstance(v, str, repr(d))


class CalculateValueTests(TestCase):

    def test_tags_known(self):
        known_tags = set()
        for fixer in available_lintian_fixers():
            known_tags.update(fixer.lintian_tags)
        for tag in DEFAULT_ADDON_FIXERS:
            self.assertIn(tag, known_tags)
        for tag in LINTIAN_BRUSH_TAG_VALUES:
            self.assertIn(tag, known_tags)

    def test_simple(self):
        self.assertEqual(0, calculate_value(set()))
        self.assertEqual(
            DEFAULT_VALUE_LINTIAN_BRUSH + LINTIAN_BRUSH_TAG_DEFAULT_VALUE,
            calculate_value(['foo']))
        self.assertEqual(
            DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY
            + LINTIAN_BRUSH_TAG_DEFAULT_VALUE,
            calculate_value([DEFAULT_ADDON_FIXERS[0]]))
