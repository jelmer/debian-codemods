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

import unittest

core_names = [
    "changelog",
    "config",
    "debhelper",
    "detect_gbp_dch",
    "lintian",
    "lintian_overrides",
    "multiarch_hints",
    "patches",
    "run",
    "salsa",
    "scrub_obsolete",
    "systemd",
    "vcs",
    "xdg",
    "yaml",
]


def core_test_suite():
    module_names = [__name__ + ".test_" + name for name in core_names]
    loader = unittest.TestLoader()
    return loader.loadTestsFromNames(module_names)


def test_suite():
    module_names = [__name__ + ".test_" + name for name in core_names]
    module_names.append(__name__ + ".fixers.test_suite")
    loader = unittest.TestLoader()
    return loader.loadTestsFromNames(module_names)
