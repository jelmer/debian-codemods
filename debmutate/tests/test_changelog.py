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

import os
import shutil
import tempfile

from debmutate.changelog import (
    ChangelogCreateError,
    ChangelogEditor,
    TextWrapper,
    rewrap_change,
    _inc_version,
    changes_sections,
    )

from debian.changelog import Version

from unittest import TestCase


class UpdateChangelogTests(TestCase):

    def setUp(self):
        self.test_dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.test_dir)
        self.addCleanup(os.chdir, os.getcwd())
        os.chdir(self.test_dir)

    def test_edit_simple(self):
        os.mkdir('debian')
        with open('debian/changelog', 'w') as f:
            f.write("""\
lintian-brush (0.28) UNRELEASED; urgency=medium

  * Add fixer for obsolete-runtime-tests-restriction.

 -- Jelmer Vernooij <jelmer@debian.org>  Mon, 02 Sep 2019 00:23:11 +0000
""")

        with ChangelogEditor() as updater:
            updater.changelog.version = '0.29'
        with open('debian/changelog', 'r') as f:
            self.assertEqual("""\
lintian-brush (0.29) UNRELEASED; urgency=medium

  * Add fixer for obsolete-runtime-tests-restriction.

 -- Jelmer Vernooij <jelmer@debian.org>  Mon, 02 Sep 2019 00:23:11 +0000
""", f.read())

    def test_invalid(self):
        os.mkdir('debian')
        with open('debian/changelog', 'w') as f:
            f.write("""\
lalalalala
""")
        self.assertRaises(ChangelogCreateError, ChangelogEditor().__enter__)


class TextWrapperTests(TestCase):

    def setUp(self):
        super(TextWrapperTests, self).setUp()
        self.wrapper = TextWrapper()

    def test_wrap_closes(self):
        self.assertEqual(
            ['And', ' ', 'this', ' ', 'fixes', ' ', 'something.', ' ',
             'Closes: #123456'],
            self.wrapper._split('And this fixes something. Closes: #123456'))


LONG_LINE = (
    "This is a very long line that could have been broken "
    "and should have been broken but was not broken.")


class RewrapChangeTests(TestCase):

    def test_too_short(self):
        self.assertEqual([], rewrap_change([]))
        self.assertEqual(['Foo bar'], rewrap_change(['Foo bar']))
        self.assertEqual(['Foo', 'bar'], rewrap_change(['Foo', 'bar']))
        self.assertEqual(
            ['  * Beginning', '  next line'],
            rewrap_change(
                ['  * Beginning', '  next line']))

    def test_no_initial(self):
        self.assertEqual(['x' * 100], rewrap_change(['x' * 100]))

    def test_wrap(self):
        self.assertEqual(
            ['  * This is a very long line that could have been '
             'broken and should have been',
             '    broken but was not broken.'],
            rewrap_change(['  * ' + LONG_LINE]))
        self.assertEqual("""\
  * Build-Depend on libsdl1.2-dev, libsdl-ttf2.0-dev and libsdl-mixer1.2-dev
    instead of with the embedded version, add -lSDL_ttf to --with-py-libs in
    debian/rules and rebootstrap (Closes: #382202)
""".splitlines(), rewrap_change("""\
  * Build-Depend on libsdl1.2-dev, libsdl-ttf2.0-dev and libsdl-mixer1.2-dev \
instead
    of with the embedded version, add -lSDL_ttf to --with-py-libs in \
debian/rules
    and rebootstrap (Closes: #382202)
""".splitlines()))

    def test_no_join(self):
        self.assertEqual("""\
    - Translators know why this sign has been put here:
        _Choices: ${FOO}, !Other[ You only have to translate Other, remove the
      exclamation mark and this comment between brackets]
      Currently text, newt, slang and gtk frontends support this feature.
""".splitlines(), rewrap_change("""\
    - Translators know why this sign has been put here:
        _Choices: ${FOO}, !Other[ You only have to translate Other, remove \
the exclamation mark and this comment between brackets]
      Currently text, newt, slang and gtk frontends support this feature.
""".splitlines()))


class IncVersionTests(TestCase):

    def test_native(self):
        self.assertEqual(Version('1.1'), _inc_version(Version('1.0')))
        self.assertEqual(Version('1a1.1'), _inc_version(Version('1a1.0')))
        self.assertEqual(Version('9.11~2'), _inc_version(Version('9.11~1')))
        self.assertEqual(Version('9.11~1'), _inc_version(Version('9.11~')))

    def test_non_native(self):
        self.assertEqual(Version('1.1-2'), _inc_version(Version('1.1-1')))
        self.assertEqual(Version('9.11-1~1'), _inc_version(Version('9.11-1~')))
        self.assertEqual(
            Version('9.11-1~2'), _inc_version(Version('9.11-1~1')))


class ChangesSectionsTests(TestCase):

    def test_simple(self):
        self.assertEqual([
            (None, [1, 2, 3, 4], [
                ([(1, '  * Change 1')]),
                ([(2, '  * Change 2'), (3, '    rest')]),
            ])], list(changes_sections([
                '',
                '  * Change 1',
                '  * Change 2',
                '    rest',
                ''
                ])))

    def test_with_header(self):
        self.assertEqual([
            ('Author 1', [1, 2, 3], [([(2, '  * Change 1')])]),
            ('Author 2', [4, 5, 6, 7], [([(5, '  * Change 2'),
                                          (6, '    rest')])]),
            ], list(changes_sections([
                '',
                '  [ Author 1 ]',
                '  * Change 1',
                '',
                '  [ Author 2 ]',
                '  * Change 2',
                '    rest',
                '',
                ])))
