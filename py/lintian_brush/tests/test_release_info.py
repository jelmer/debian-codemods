#!/usr/bin/python3
# Copyright (C) 2024 Jelmer Vernooij
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

"""Tests for the release_info module."""

from unittest import TestCase

from debian.changelog import Version

from lintian_brush.release_info import key_package_version


class KeyPackageVersionTests(TestCase):
    def test_dpkg_version(self):
        # Test getting dpkg version for a release
        version = key_package_version("dpkg", "bullseye")
        if version is not None:
            self.assertIsInstance(version, Version)

    def test_debhelper_version(self):
        # Test getting debhelper version for a release
        version = key_package_version("debhelper", "bullseye")
        if version is not None:
            self.assertIsInstance(version, Version)

    def test_unknown_package(self):
        # Test with unknown package
        with self.assertRaises(ValueError) as cm:
            key_package_version("unknown-package", "bullseye")
        self.assertEqual(
            str(cm.exception),
            "Unknown package 'unknown-package' for release 'bullseye'",
        )

    def test_none_for_unknown_release(self):
        # Test with unknown release (should return None)
        version = key_package_version("dpkg", "unknown-release")
        self.assertIsNone(version)

        version = key_package_version("debhelper", "unknown-release")
        self.assertIsNone(version)
