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

"""Tests for lintian_brush.rules."""

from breezy.tests import (
    TestCase,
    TestCaseWithTransport,
    )

from lintian_brush.rules import (
    Makefile,
    Rule,
    dh_invoke_drop_with,
    dh_invoke_add_with,
    matches_wildcard,
    update_rules,
    )


class MakefileParseTests(TestCase):

    def test_simple_rule(self):
        mf = Makefile.from_bytes(b"""\
all:
\ttest
""")
        self.assertEqual(mf.contents, [Rule(b'all', commands=[b'test'])])

    def test_conditional(self):
        mf = Makefile.from_bytes(b"""\
all:
ifeq (foo, bar)
\ttest
  endif
""")
        self.assertEqual(1, len(mf.contents))

    def test_conditional_rule(self):
        mf = Makefile.from_bytes(b"""\
all: %: test
\ttest
""")
        self.assertEqual(
            mf.contents,
            [Rule(b'all', commands=[b'test'], prereq_targets=[b'%:', b'test'])]
            )


class UpdateRulesTests(TestCaseWithTransport):

    def test_update_command(self):
        self.build_tree_contents([('debian/', ), ('debian/rules', """\
SOMETHING = 1

all:
\techo blah
\techo foo
""")])

        def replace(line, target):
            if line == b'echo blah':
                return b'echo bloe'
            return line
        self.assertTrue(update_rules(replace))
        self.assertFalse(update_rules(replace))
        self.assertFileEqual("""\
SOMETHING = 1

all:
\techo bloe
\techo foo
""", 'debian/rules')

    def test_continuation(self):
        self.build_tree_contents([('debian/', ), ('debian/rules', """\
SOMETHING = 1

all:
\techo blah \\
foo
""")])

        def replace(line, target):
            if line == b'echo blah \\\nfoo':
                return b'echo bloe'
            return line
        self.assertTrue(update_rules(replace))
        self.assertFalse(update_rules(replace))
        self.assertFileEqual("""\
SOMETHING = 1

all:
\techo bloe
""", 'debian/rules')

    def test_empty_line(self):
        self.build_tree_contents([('debian/', ), ('debian/rules', """\
SOMETHING = 1

all:
\techo blah

\techo foo
""")])

        def replace(line, target):
            if line == b'echo foo':
                return b'echo bloe'
            return line
        self.assertTrue(update_rules(replace))
        self.assertFalse(update_rules(replace))
        self.assertFileEqual("""\
SOMETHING = 1

all:
\techo blah

\techo bloe
""", 'debian/rules')

    def test_keep_rule_cb(self):
        self.build_tree_contents([('debian/', ), ('debian/rules', """\
SOMETHING = 1

all:
\techo blah

none:
\techo foo
""")])

        def discard_none(rule):
            if rule.target == b'none':
                rule.clear()
        self.assertTrue(update_rules(rule_cb=discard_none))
        self.assertFalse(update_rules(rule_cb=discard_none))
        self.assertFileEqual("""\
SOMETHING = 1

all:
\techo blah
""", 'debian/rules')


class MakefileTests(TestCase):

    def test_add_rule(self):
        mf = Makefile.from_bytes(b"""\
SOMETHING = 1

all:
\techo blah

none:
\techo foo
""")
        r = mf.add_rule(b'blah')
        self.assertIsInstance(r, Rule)
        r.append_command(b'echo really blah')
        self.assertEqual(b"""\
SOMETHING = 1

all:
\techo blah

none:
\techo foo

blah:
\techo really blah

""", mf.dump())


class InvokeDropWithTests(TestCaseWithTransport):

    def test_drop_with(self):
        self.assertEqual(
            b'dh',
            dh_invoke_drop_with(b'dh --with=blah', b'blah'))
        self.assertEqual(
            b'dh --with=foo',
            dh_invoke_drop_with(b'dh --with=blah,foo', b'blah'))
        self.assertEqual(
            b'dh --with=foo --other',
            dh_invoke_drop_with(b'dh --with=blah,foo --other', b'blah'))
        self.assertEqual(
            b'dh',
            dh_invoke_drop_with(b'dh --with=blah', b'blah'))
        self.assertEqual(
            b'dh --with=foo',
            dh_invoke_drop_with(b'dh --with=foo,blah', b'blah'))
        self.assertEqual(
            b'dh $@ --verbose --with=autoreconf,cme-upgrade',
            dh_invoke_drop_with(
                b'dh $@ --verbose --with autoreconf,systemd,cme-upgrade',
                b'systemd'))


class InvokeAddWithTests(TestCaseWithTransport):

    def test_add_with(self):
        self.assertEqual(
            b'dh --with=blah',
            dh_invoke_add_with(b'dh', b'blah'))
        self.assertEqual(
            b'dh --with=foo,blah',
            dh_invoke_add_with(b'dh --with=foo', b'blah'))
        self.assertEqual(
            b'dh --with=foo,blah --other',
            dh_invoke_add_with(b'dh --with=foo --other', b'blah'))


class MatchesWildcardTests(TestCase):

    def test_some(self):
        self.assertTrue(matches_wildcard('foo', 'foo'))
        self.assertTrue(matches_wildcard('foo', 'fo%'))
        self.assertTrue(matches_wildcard('foo', '%'))
        self.assertFalse(matches_wildcard('foo', 'bar'))
        self.assertFalse(matches_wildcard('foo', 'fo'))
        self.assertFalse(matches_wildcard('foo', 'oo'))
        self.assertFalse(matches_wildcard('foo', 'b%'))
