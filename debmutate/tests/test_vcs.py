#!/usr/bin/python3
# Copyright (C) 2018-2020 Jelmer Vernooij
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

"""Tests for the vcs module."""

from unittest import TestCase

from lintian_brush.vcs import (
    split_vcs_url,
    unsplit_vcs_url,
    )


class SplitVcsUrlTests(TestCase):

    def test_none(self):
        self.assertEqual(
            ('https://github.com/jelmer/example', None, None),
            split_vcs_url('https://github.com/jelmer/example'))
        self.assertEqual(
            ('https://github.com/jelmer/example', None, 'path/to/packaging'),
            split_vcs_url(
                'https://github.com/jelmer/example [path/to/packaging]'))

    def test_branch(self):
        self.assertEqual(
            ('https://github.com/jelmer/example',
                'master', 'path/to/packaging'),
            split_vcs_url(
                'https://github.com/jelmer/example [path/to/packaging] '
                '-b master'))
        self.assertEqual(
            ('https://github.com/jelmer/example',
                'master', 'path/to/packaging'),
            split_vcs_url(
                'https://github.com/jelmer/example -b master '
                '[path/to/packaging]'))
        self.assertEqual(
            ('https://github.com/jelmer/example', 'master', None),
            split_vcs_url(
                'https://github.com/jelmer/example -b master'))


class UnsplitVcsUrlTests(TestCase):

    def test_none(self):
        self.assertEqual(
            'https://github.com/jelmer/example',
            unsplit_vcs_url('https://github.com/jelmer/example', None, None))
        self.assertEqual(
            'https://github.com/jelmer/example [path/to/packaging]',
            unsplit_vcs_url(
                'https://github.com/jelmer/example', None,
                'path/to/packaging'))

    def test_branch(self):
        self.assertEqual(
            'https://github.com/jelmer/example -b master '
            '[path/to/packaging]',
            unsplit_vcs_url(
                'https://github.com/jelmer/example', 'master',
                'path/to/packaging'))
        self.assertEqual(
            'https://github.com/jelmer/example -b master',
            unsplit_vcs_url(
                'https://github.com/jelmer/example', 'master', None))
