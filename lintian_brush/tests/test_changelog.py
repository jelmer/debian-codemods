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

from datetime import datetime

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush.changelog import (
    ChangelogCreateError,
    ChangelogUpdater,
    TextWrapper,
    rewrap_change,
    add_changelog_entry,
    _inc_version,
    )

from debian.changelog import Version


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


class ChangelogAddEntryTests(TestCaseWithTransport):

    def test_edit_existing_new_author(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Initial change.
  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', ['Add a foo'])
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Joe Example ]
  * Initial change.
  * Support updating templated debian/control files that use cdbs
    template.

  [ Jane Example ]
  * Add a foo

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_edit_existing_multi_new_author(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Jane Example ]
  * Support updating templated debian/control files that use cdbs
    template.

  [ Joe Example ]
  * Another change

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', ['Add a foo'])
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Jane Example ]
  * Support updating templated debian/control files that use cdbs
    template.

  [ Joe Example ]
  * Another change

  [ Jane Example ]
  * Add a foo

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_edit_existing_existing_author(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Joe Example')
        self.overrideEnv('DEBEMAIL', 'joe@example.com')
        add_changelog_entry(tree, 'debian/changelog', ['Add a foo'])
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.
  * Add a foo

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_add_new(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) unstable; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        self.overrideEnv('DEBCHANGE_VENDOR', 'debian')
        add_changelog_entry(tree, 'debian/changelog', ['Add a foo'],
                            timestamp=datetime(2020, 5, 24, 15, 27, 26))
        self.assertFileEqual("""\
lintian-brush (0.36) UNRELEASED; urgency=low

  * Add a foo

 -- Jane Example <jane@example.com>  Sun, 24 May 2020 15:27:26 -0000

lintian-brush (0.35) unstable; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_edit_broken_first_line(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
THIS IS NOT A PARSEABLE LINE
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Jane Example')
        self.overrideEnv('DEBEMAIL', 'jane@example.com')
        add_changelog_entry(tree, 'debian/changelog', ['Add a foo', '+ Bar'])
        self.assertFileEqual("""\
THIS IS NOT A PARSEABLE LINE
lintian-brush (0.35) UNRELEASED; urgency=medium

  [ Joe Example ]
  * Support updating templated debian/control files that use cdbs
    template.

  [ Jane Example ]
  * Add a foo
    + Bar

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_add_long_line(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Joe Example')
        self.overrideEnv('DEBEMAIL', 'joe@example.com')
        add_changelog_entry(
            tree, 'debian/changelog', [
            'This is adding a very long sentence that is longer than '
            'would fit on a single line in a 80-character-wide line.'])
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.
  * This is adding a very long sentence that is longer than would fit on a
    single line in a 80-character-wide line.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')

    def test_add_long_subline(self):
        tree = self.make_branch_and_tree('.')
        self.build_tree_contents([
            ('debian/', ),
            ('debian/changelog', """\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""")])
        tree.add(['debian', 'debian/changelog'])
        self.overrideEnv('DEBFULLNAME', 'Joe Example')
        self.overrideEnv('DEBEMAIL', 'joe@example.com')
        add_changelog_entry(
            tree, 'debian/changelog', [
            'This is the main item.',
            '+ This is adding a very long sentence that is longer than '
            'would fit on a single line in a 80-character-wide line.'])
        self.assertFileEqual("""\
lintian-brush (0.35) UNRELEASED; urgency=medium

  * Support updating templated debian/control files that use cdbs
    template.
  * This is the main item.
    + This is adding a very long sentence that is longer than would fit on a
      single line in a 80-character-wide line.

 -- Joe Example <joe@example.com>  Fri, 04 Oct 2019 02:36:13 +0000
""", 'debian/changelog')


class IncVersionTests(TestCase):

    def test_native(self):
        self.assertEqual(Version('1.1'), _inc_version(Version('1.0')))
        self.assertEqual(Version('1a1.1'), _inc_version(Version('1a1.0')))

    def test_non_native(self):
        self.assertEqual(Version('1.1-2'), _inc_version(Version('1.1-1')))
