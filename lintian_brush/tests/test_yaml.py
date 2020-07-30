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

"""Tests for lintian_brush.yaml."""

from breezy.tests import (
    TestCaseInTempDir,
    TestCase,
    )

from ruamel.yaml.compat import ordereddict

from lintian_brush.yaml import (
    YamlUpdater,
    update_ordered_dict,
    )


class YamlUpdaterTests(TestCaseInTempDir):

    def test_new(self):
        with YamlUpdater('newfile.yaml') as editor:
            editor.code['Somekey'] = 'Somevalue'
        self.assertFileEqual("---\nSomekey: Somevalue\n", "newfile.yaml")

    def test_update(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
Origkey: origvalue
Somekey: origvalue
""")
        with YamlUpdater('newfile.yaml') as editor:
            editor.code['Somekey'] = 'Somevalue'
        self.assertFileEqual("""\
Origkey: origvalue
Somekey: Somevalue
""", "newfile.yaml")

    def test_delete(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
Origkey: origvalue
Somekey: origvalue
""")
        with YamlUpdater('newfile.yaml') as editor:
            del editor.code['Origkey']
            del editor.code['Somekey']
        self.assertPathDoesNotExist('newfile.yaml')

    def test_no_change(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
Origkey: origvalue
Somekey: origvalue
""")
        with YamlUpdater('newfile.yaml'):
            pass
        self.assertFileEqual("""\
Origkey: origvalue
Somekey: origvalue
""", "newfile.yaml")

    def test_preserve_header(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
---
Origkey: origvalue
Somekey: origvalue
""")
        with YamlUpdater('newfile.yaml') as editor:
            editor.code['Newkey'] = 'newvalue'
        self.assertFileEqual("""\
---
Origkey: origvalue
Somekey: origvalue
Newkey: newvalue
""", "newfile.yaml")

    def test_interrupted_line(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
---
Origkey: origvalue
Somekey: origvalue""")
        with YamlUpdater('newfile.yaml') as editor:
            editor.code['Newkey'] = 'newvalue'
        self.assertFileEqual("""\
---
Origkey: origvalue
Somekey: origvalue
Newkey: newvalue
""", "newfile.yaml")


class UpdateOrderedDict(TestCase):

    def setUp(self):
        super(UpdateOrderedDict, self).setUp()
        self._od = ordereddict()

    def test_empty(self):
        update_ordered_dict(self._od, [('Contact', 'Foo'), ('Blah', 'blah')])
        self.assertEqual(ordereddict([
            ('Blah', 'blah'),
            ('Contact', 'Foo')]), self._od)

    def test_modify(self):
        self._od['Contact'] = 'Bar'
        self._od['ZZ'] = 'z'
        update_ordered_dict(
            self._od, [('Contact', 'Foo'), ('Blah', 'blah')])
        self.assertEqual(ordereddict([
            ('Blah', 'blah'),
            ('Contact', 'Foo'),
            ('ZZ', 'z'),
            ]), self._od)

    def test_insert_before(self):
        self._od['Contact'] = 'Bar'
        self._od['Bar'] = 'Bar'
        update_ordered_dict(
            self._od, [('Daar', 'blah')])
        self.assertEqual(ordereddict([
            ('Contact', 'Bar'),
            ('Bar', 'Bar'),
            ('Daar', 'blah'),
            ]), self._od)

    def test_csb(self):
        from ..upstream_metadata import upstream_metadata_sort_key
        self._od['Registry'] = [
                ordereddict([('Name', 'OMICtools'), ('Entry', 'OMICS_09827')]),
                ordereddict([('Name', 'bio.tools'), ('Entry', 'NA')])]
        self._od['Repository'] = 'https://github.com/csb-toolbox/CSB'
        update_ordered_dict(
            self._od,
            [('Bug-Database', 'https://github.com/csb-toolbox/CSB/issues'),
             ('Bug-Submit', 'https://github.com/csb-toolbox/CSB/issues/new'),
             ('Repository', 'https://github.com/csb-toolbox/CSB/issues.git')],
            key=upstream_metadata_sort_key)
        self.assertEqual(
            ordereddict([
                 ('Bug-Database', 'https://github.com/csb-toolbox/CSB/issues'),
                 ('Bug-Submit',
                  'https://github.com/csb-toolbox/CSB/issues/new'),
                 ('Registry', [
                     ordereddict([('Name', 'OMICtools'),
                                  ('Entry', 'OMICS_09827')]),
                     ordereddict([('Name', 'bio.tools'), ('Entry', 'NA')])
                  ]),
                 ('Repository',
                  'https://github.com/csb-toolbox/CSB/issues.git')]), self._od)
