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
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush.changelog import (
    ChangelogCreateError,
    ChangelogUpdater,
    TextWrapper,
    rewrap_change,
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
