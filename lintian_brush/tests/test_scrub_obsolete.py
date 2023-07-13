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

"""Tests for lintian_brush.scrub_obsolete."""

from typing import Dict
from unittest import TestCase

from debmutate._deb822 import PkgRelation

from debian.changelog import Version
from lintian_brush.scrub_obsolete import (
    DropEssential,
    DropMinimumVersion,
    DropTransition,
    PackageChecker,
    ReplaceTransition,
    drop_obsolete_depends,
    filter_relations,
    name_list,
    release_aliases,
)


class NameListTests(TestCase):

    def test_empty(self):
        self.assertRaises(ValueError, name_list, [])

    def test_some(self):
        self.assertEqual('foo', name_list(['foo']))
        self.assertEqual('foo', name_list(['foo', 'foo', 'foo']))
        self.assertEqual(
            'bar and foo', name_list(['foo', 'bar', 'foo']))
        self.assertEqual(
            'bar, bla and foo', name_list(['foo', 'bar', 'foo', 'bla']))


class FilterRelationsTests(TestCase):

    def test_missing(self):
        control: Dict[str, str] = {}
        self.assertEqual(
            [],
            filter_relations(control, "Build-Depends", None))  # type: ignore

    def test_keep(self):
        control = {"Depends": "foo"}

        def cb(oldrel):
            return oldrel, []

        self.assertEqual([], filter_relations(control, "Depends", cb))

    def test_drop_last(self):
        control = {"Depends": "foo"}

        def cb(oldrel):
            return [], oldrel

        self.assertEqual(PkgRelation.parse("foo"),
                         filter_relations(control, "Depends", cb))
        self.assertEqual({}, control)

    def test_drop(self):
        control = {"Depends": "foo, bar"}

        def cb(oldrel):
            if oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(PkgRelation.parse("foo"),
                         filter_relations(control, "Depends", cb))
        self.assertEqual({"Depends": "bar"}, control)

    def test_keep_last_comma(self):
        control = {"Depends": "foo, bar, "}

        def cb(oldrel):
            if oldrel and oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(PkgRelation.parse("foo"),
                         filter_relations(control, "Depends", cb))
        self.assertEqual({"Depends": "bar, "}, control)

    def test_drop_just_comma(self):
        control = {"Depends": "foo, "}

        def cb(oldrel):
            if oldrel and oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(PkgRelation.parse("foo"),
                         filter_relations(control, "Depends", cb))
        self.assertEqual({}, control)


class DummyChecker(PackageChecker):

    release = "release"

    def __init__(self, versions, essential, *, transitions=None):
        self.versions = versions
        self.essential = essential
        self._transitions = transitions or {}

    def package_version(self, package):
        return self.versions.get(package)

    def is_essential(self, package):
        return package in self.essential


class DropObsoleteDependsTests(TestCase):

    def test_empty(self):
        self.assertEqual(
            ([], []), drop_obsolete_depends([], DummyChecker({}, [])))

    def test_single(self):
        checker = DummyChecker({'simple': Version('1.1')}, {})
        orig = PkgRelation.parse('simple (>= 1.0)')
        self.assertEqual(
            (PkgRelation.parse('simple'),
             [DropMinimumVersion(*PkgRelation.parse('simple (>= 1.0)'))]),
            drop_obsolete_depends(orig, checker))

    def test_essential(self):
        checker = DummyChecker({'simple': Version('1.1')}, {'simple'})
        orig = PkgRelation.parse('simple (>= 1.0)')
        self.assertEqual(
            ([], [DropEssential(*PkgRelation.parse('simple (>= 1.0)'))]),
            drop_obsolete_depends(orig, checker))

    def test_debhelper(self):
        checker = DummyChecker({'debhelper': Version('1.4')}, {})
        orig = PkgRelation.parse('debhelper (>= 1.1)')
        self.assertEqual(
            (PkgRelation.parse('debhelper (>= 1.1)'), []),
            drop_obsolete_depends(orig, checker))

    def test_other_essential(self):
        checker = DummyChecker({'simple': Version('1.1')}, {'simple'})
        orig = PkgRelation.parse('simple (>= 1.0) | other')
        self.assertEqual(
            ([],
             [DropEssential(*PkgRelation.parse('simple (>= 1.0)'))]),
            drop_obsolete_depends(orig, checker))

    def test_transition(self):
        checker = DummyChecker({'simple': Version('1.1')}, {'simple'},
                               transitions={'oldpackage': 'replacement'})
        orig = PkgRelation.parse('oldpackage (>= 1.0) | other')
        self.assertEqual(
            (PkgRelation.parse('replacement | other'),
             [ReplaceTransition(PkgRelation.parse('oldpackage (>= 1.0)')[0],
                                PkgRelation.parse('replacement'))]),
            drop_obsolete_depends(orig, checker))

    def test_transition_matches(self):
        checker = DummyChecker({'simple': Version('1.1')}, {'simple'},
                               transitions={'oldpackage': 'replacement'})
        orig = PkgRelation.parse('oldpackage (>= 1.0) | replacement ')
        self.assertEqual(
            (PkgRelation.parse('replacement'),
             [DropTransition(*PkgRelation.parse('oldpackage (>= 1.0)'))]),
            drop_obsolete_depends(orig, checker))

    def test_transition_dupes(self):
        checker = DummyChecker({'simple': Version('1.1')}, {'simple'},
                               transitions={'oldpackage': 'replacement'})
        orig = PkgRelation.parse(
            'oldpackage (>= 1.0) | oldpackage (= 3.0) | other')
        self.assertEqual(
            (PkgRelation.parse('replacement | other'),
             [ReplaceTransition(PkgRelation.parse('oldpackage (>= 1.0)')[0],
                                PkgRelation.parse('replacement')),
              ReplaceTransition(PkgRelation.parse('oldpackage (= 3.0)')[0],
                                PkgRelation.parse('replacement'))]),
            drop_obsolete_depends(orig, checker))


class ReleaseAliasesTests(TestCase):

    def test_existing(self):
        self.assertEqual('(unstable)', release_aliases('sid'))

    def test_missing(self):
        self.assertEqual('', release_aliases('unknown'))
