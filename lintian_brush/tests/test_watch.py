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

"""Tests for lintian_brush.watch."""

from breezy.tests import (
    TestCase,
    )

from io import StringIO

from lintian_brush.watch import (
    parse_watch_file,
    MissingVersion,
    Watch,
    )


class ParseWatchFileTests(TestCase):

    def test_parse_empty(self):
        self.assertIs(None, parse_watch_file(StringIO("")))

    def test_parse_no_version(self):
        self.assertRaises(
            MissingVersion, parse_watch_file, StringIO("foo\n"))
        self.assertRaises(
            MissingVersion, parse_watch_file, StringIO("foo=bar\n"))

    def test_parse_simple(self):
        wf = parse_watch_file(StringIO("""\
version=4
https://samba.org/~jelmer/
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual([Watch('https://samba.org/~jelmer/')], wf.entries)

    def test_parse_with_opts(self):
        wf = parse_watch_file(StringIO("""\
version=4
opts=pgpmode=mangle https://samba.org/~jelmer/
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual([], wf.options)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/', opts=['pgpmode=mangle'])],
            wf.entries)

    def test_parse_global_opts(self):
        wf = parse_watch_file(StringIO("""\
version=4
opts=pgpmode=mangle
https://samba.org/~jelmer/
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(['pgpmode=mangle'], wf.options)
        self.assertEqual([Watch('https://samba.org/~jelmer/')], wf.entries)
