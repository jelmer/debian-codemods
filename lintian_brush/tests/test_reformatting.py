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

"""Tests for lintian brush reformatting tools."""

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush.reformatting import (
    FormattingUnpreservable,
    check_generated_file,
    check_preserve_formatting,
    GeneratedControlFile,
    )


class CheckPreserveFormattingTests(TestCase):

    def test_formatting_same(self):
        check_preserve_formatting("FOO  ", "FOO  ", 'debian/blah')

    def test_formatting_different(self):
        self.assertRaises(
            FormattingUnpreservable,
            check_preserve_formatting, "FOO ", "FOO  ", 'debian/blah')

    def test_reformatting_allowed(self):
        self.overrideEnv('REFORMATTING', 'allow')
        check_preserve_formatting("FOO  ", "FOO ", 'debian/blah')


class GeneratedFileTests(TestCaseWithTransport):

    def test_generated_control_file(self):
        self.build_tree_contents([
            ('debian/', ),
            ('debian/control.in', """\
Source: blah
""")])
        self.assertRaises(
            GeneratedControlFile, check_generated_file, 'debian/control')
