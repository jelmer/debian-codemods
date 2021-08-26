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

from unittest import TestCase

from lintian_brush.scrub_obsolete import name_list, filter_relations


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
        control = {}
        self.assertEqual(
            [], filter_relations(control, "Build-Depends", None))

    def test_keep(self):
        control = {"Depends": "foo"}

        def cb(oldrel):
            return oldrel, []

        self.assertEqual([], filter_relations(control, "Depends", cb))

    def test_drop_last(self):
        control = {"Depends": "foo"}

        def cb(oldrel):
            return [], oldrel

        self.assertEqual(["foo"], filter_relations(control, "Depends", cb))
        self.assertEqual({}, control)

    def test_drop(self):
        control = {"Depends": "foo, bar"}

        def cb(oldrel):
            if oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(["foo"], filter_relations(control, "Depends", cb))
        self.assertEqual({"Depends": "bar"}, control)

    def test_keep_last_comma(self):
        control = {"Depends": "foo, bar, "}

        def cb(oldrel):
            if oldrel and oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(["foo"], filter_relations(control, "Depends", cb))
        self.assertEqual({"Depends": "bar, "}, control)

    def test_drop_just_comma(self):
        control = {"Depends": "foo, "}

        def cb(oldrel):
            if oldrel and oldrel[0].name == 'foo':
                return [], oldrel
            return oldrel, []

        self.assertEqual(["foo"], filter_relations(control, "Depends", cb))
        self.assertEqual({}, control)
