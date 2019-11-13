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

"""Tests for lintian_brush.changelog."""

from breezy.tests import (
    TestCaseWithTransport,
    )

from lintian_brush.changelog import (
    ChangelogCreateError,
    ChangelogUpdater,
    )


class UpdateChangelogTests(TestCaseWithTransport):

    def test_edit_simple(self):
        self.build_tree_contents([('debian/', ), ('debian/changelog', """\
lintian-brush (0.28) UNRELEASED; urgency=medium

  * Add fixer for obsolete-runtime-tests-restriction.

 -- Jelmer Vernooij <jelmer@debian.org>  Mon, 02 Sep 2019 00:23:11 +0000
""")])

        with ChangelogUpdater() as updater:
            updater.changelog.version = '0.29'
        self.assertFileEqual("""\
lintian-brush (0.29) UNRELEASED; urgency=medium

  * Add fixer for obsolete-runtime-tests-restriction.

 -- Jelmer Vernooij <jelmer@debian.org>  Mon, 02 Sep 2019 00:23:11 +0000
""", 'debian/changelog')

    def test_invalid(self):
        self.build_tree_contents([('debian/', ), ('debian/changelog', """\
lalalalala
""")])
        self.assertRaises(ChangelogCreateError, ChangelogUpdater().__enter__)
