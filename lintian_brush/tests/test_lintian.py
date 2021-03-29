#!/usr/bin/python
# Copyright (C) 2021 Jelmer Vernooij
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

"""Tests for lintian_brush.lintian."""

from io import StringIO

from breezy.tests import TestCase

from lintian_brush.lintian import read_list_file


class ReadListFileTests(TestCase):

    def test_read_simple(self):
        f = StringIO("""\
# this is a comment

item1
item2

# more comments
item3
""")
        self.assertEqual(
            ['item1', 'item2', 'item3'],
            list(read_list_file(f, 'debian')))

    def test_vendor(self):
        f = StringIO("""\
# this is a comment

@if-vendor-is-not ubuntu item1
@if-vendor-is-not debian item2

# more comments
item3
""")
        self.assertEqual(
            ['item1', 'item3'],
            list(read_list_file(f, 'debian')))
