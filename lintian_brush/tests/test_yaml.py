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
    )

from lintian_brush.yaml import (
    YamlUpdater,
    )


class YamlUpdaterTests(TestCaseInTempDir):

    def test_new(self):
        with YamlUpdater('newfile.yaml') as code:
            code['Somekey'] = 'Somevalue'
        self.assertFileEqual("Somekey: Somevalue\n", "newfile.yaml")

    def test_update(self):
        with open('newfile.yaml', 'w') as f:
            f.write("""\
Origkey: origvalue
Somekey: origvalue
""")
        with YamlUpdater('newfile.yaml') as code:
            code['Somekey'] = 'Somevalue'
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
        with YamlUpdater('newfile.yaml') as code:
            del code['Origkey']
            del code['Somekey']
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
        with YamlUpdater('newfile.yaml') as code:
            code['Newkey'] = 'newvalue'
        self.assertFileEqual("""\
---
Origkey: origvalue
Somekey: origvalue
Newkey: newvalue
""", "newfile.yaml")
