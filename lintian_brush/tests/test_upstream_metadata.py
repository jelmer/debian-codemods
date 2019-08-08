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

"""Tests for lintian_brush.upstream_metadata."""

from breezy.tests import (
    TestCaseWithTransport,
    )


from lintian_brush.upstream_metadata import (
    guess_from_package_json,
    )


class GuessFromPackageJsonTests(TestCaseWithTransport):

    def test_simple(self):
        self.build_tree_contents([('package.json', """\
{
  "name": "autosize",
  "version": "4.0.2",
  "author": {
    "name": "Jack Moore",
    "url": "http://www.jacklmoore.com",
    "email": "hello@jacklmoore.com"
  },
  "main": "dist/autosize.js",
  "license": "MIT",
  "homepage": "http://www.jacklmoore.com/autosize",
  "demo": "http://www.jacklmoore.com/autosize",
  "repository": {
    "type": "git",
    "url": "http://github.com/jackmoore/autosize.git"
  }
}
""")])
        self.assertEqual(
            [('Name', 'autosize', 'certain'),
             ('Homepage', 'http://www.jacklmoore.com/autosize', 'certain'),
             ('Repository', 'http://github.com/jackmoore/autosize.git',
              'certain')],
            list(guess_from_package_json('package.json', False)))

    def test_dummy(self):
        self.build_tree_contents([('package.json', """\
{
  "name": "mozillaeslintsetup",
  "description": "This package file is for setup of ESLint.",
  "repository": {},
  "license": "MPL-2.0",
  "dependencies": {
    "eslint": "4.18.1",
    "eslint-plugin-html": "4.0.2",
    "eslint-plugin-mozilla": "file:tools/lint/eslint/eslint-plugin-mozilla",
    "eslint-plugin-no-unsanitized": "2.0.2",
    "eslint-plugin-react": "7.1.0",
    "eslint-plugin-spidermonkey-js":
        "file:tools/lint/eslint/eslint-plugin-spidermonkey-js"
  },
  "devDependencies": {}
}
""")])
        self.assertEqual(
            [('Name', 'mozillaeslintsetup', 'certain')],
            list(guess_from_package_json('package.json', False)))
