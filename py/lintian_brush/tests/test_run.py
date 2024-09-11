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
import re

from breezy.tests import (
    TestCase,
)

from debian.changelog import (
    Changelog,
)
from lintian_brush import (
    certainty_sufficient,
    certainty_to_confidence,
    min_certainty,
    version_string,
)


class LintianBrushVersion(TestCase):
    def test_matches_package_version(self):
        if not os.path.exists("debian/changelog"):
            self.skipTest(
                "no debian/changelog available. "
                "Running outside of source tree?"
            )
        with open("debian/changelog") as f:
            cl = Changelog(f, max_blocks=1)
        package_version = str(cl.version)
        m = re.match(r"^\d+\.\d+", package_version)
        assert m is not None
        package_version = m.group(0)
        self.assertEqual(package_version, version_string)


class CertaintySufficientTests(TestCase):
    def test_sufficient(self):
        self.assertTrue(certainty_sufficient("certain", "certain"))
        self.assertTrue(certainty_sufficient("certain", "possible"))
        self.assertTrue(certainty_sufficient("certain", None))
        self.assertTrue(certainty_sufficient("possible", None))
        # TODO(jelmer): Should we really always allow unknown certainties
        # through?
        self.assertTrue(certainty_sufficient(None, "certain"))  # type: ignore

    def test_insufficient(self):
        self.assertFalse(certainty_sufficient("possible", "certain"))


class CertaintyVsConfidenceTests(TestCase):
    def test_certainty_to_confidence(self):
        self.assertEqual(0, certainty_to_confidence("certain"))
        self.assertEqual(1, certainty_to_confidence("confident"))
        self.assertEqual(2, certainty_to_confidence("likely"))
        self.assertEqual(3, certainty_to_confidence("possible"))
        self.assertIs(None, certainty_to_confidence("unknown"))
        self.assertRaises(ValueError, certainty_to_confidence, "blah")


class MinimumCertaintyTests(TestCase):
    def test_minimum(self):
        self.assertEqual("certain", min_certainty([]))
        self.assertEqual("certain", min_certainty(["certain"]))
        self.assertEqual("possible", min_certainty(["possible"]))
        self.assertEqual("possible", min_certainty(["possible", "certain"]))
        self.assertEqual("likely", min_certainty(["likely", "certain"]))
        self.assertEqual(
            "possible", min_certainty(["likely", "certain", "possible"])
        )
