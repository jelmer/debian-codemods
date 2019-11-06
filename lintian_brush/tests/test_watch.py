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

    def test_parse_with_spacing_around_version(self):
        wf = parse_watch_file(StringIO("""\
version = 3
https://samba.org/~jelmer/ blah-(\\d+).tar.gz
"""))
        self.assertEqual(3, wf.version)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/', 'blah-(\\d+).tar.gz')],
            wf.entries)

    def test_parse_with_script(self):
        wf = parse_watch_file(StringIO("""\
version=4
https://samba.org/~jelmer/ blah-(\\d+).tar.gz debian sh blah.sh
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/', 'blah-(\\d+).tar.gz',
                   'debian', 'sh blah.sh')],
            wf.entries)

    def test_parse_single(self):
        wf = parse_watch_file(StringIO("""\
version=4
https://samba.org/~jelmer/blah-(\\d+).tar.gz
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer', 'blah-(\\d+).tar.gz')],
            wf.entries)

    def test_parse_simple(self):
        wf = parse_watch_file(StringIO("""\
version=4
https://samba.org/~jelmer/ blah-(\\d+).tar.gz
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/', 'blah-(\\d+).tar.gz')],
            wf.entries)

    def test_parse_with_opts(self):
        wf = parse_watch_file(StringIO("""\
version=4
opts=pgpmode=mangle https://samba.org/~jelmer/ blah-(\\d+).tar.gz
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual([], wf.options)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/',
                   'blah-(\\d+).tar.gz', opts=['pgpmode=mangle'])],
            wf.entries)

    def test_parse_global_opts(self):
        wf = parse_watch_file(StringIO("""\
version=4
opts=pgpmode=mangle
https://samba.org/~jelmer/ blah-(\\d+).tar.gz
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(['pgpmode=mangle'], wf.options)
        self.assertEqual([
            Watch('https://samba.org/~jelmer/', 'blah-(\\d+).tar.gz')],
            wf.entries)

    def test_parse_opt_quotes(self):
        wf = parse_watch_file(StringIO("""\
version=4
opts="pgpmode=mangle" https://samba.org/~jelmer blah-(\\d+).tar.gz
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(
            wf.entries,
            [Watch('https://samba.org/~jelmer',
                   'blah-(\\d+).tar.gz', opts=['pgpmode=mangle'])])

    def test_pattern_included(self):
        wf = parse_watch_file(StringIO("""\
version=4
https://pypi.debian.net/case/case-(.+).tar.gz debian
"""))
        self.assertEqual(4, wf.version)
        self.assertEqual(
            [Watch('https://pypi.debian.net/case', 'case-(.+).tar.gz',
                   'debian')], wf.entries)

    def test_parse_weird_quotes(self):
        wf = parse_watch_file(StringIO("""\
# please also check https://pypi.debian.net/case/watch
version=3
opts=repacksuffix=+dfsg",pgpsigurlmangle=s/$/.asc/ \\
https://pypi.debian.net/case/case-(.+)\\.(?:zip|(?:tar\\.(?:gz|bz2|xz))) \\
debian sh debian/repack.stub
"""))
        self.assertEqual(3, wf.version)
        self.assertEqual([Watch(
            'https://pypi.debian.net/case',
            'case-(.+)\\.(?:zip|(?:tar\\.(?:gz|bz2|xz)))',
            'debian', 'sh debian/repack.stub',
            opts=['repacksuffix=+dfsg"', 'pgpsigurlmangle=s/$/.asc/'])],
            wf.entries)

    def test_parse_package_variable(self):
        wf = parse_watch_file(StringIO("""\
version = 3
https://samba.org/~jelmer/@PACKAGE@ blah-(\\d+).tar.gz
"""))
        self.assertEqual(3, wf.version)
        self.assertEqual(
            [Watch('https://samba.org/~jelmer/@PACKAGE@',
                   'blah-(\\d+).tar.gz')],
            wf.entries)
        self.assertEqual(
            'https://samba.org/~jelmer/blah',
            wf.entries[0].format_url('blah'))
