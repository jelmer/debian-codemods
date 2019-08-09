#!/usr/bin/python3
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

"""Tests for the vcs module."""

from unittest import TestCase

from lintian_brush.vcs import (
    fixup_broken_git_url,
    sanitize_url,
    )


class FixUpGitUrlTests(TestCase):

    def test_fixup(self):
        self.assertEqual(
            'git://github.com/jelmer/dulwich',
            fixup_broken_git_url('git://github.com:jelmer/dulwich'))

    def test_preserves(self):
        self.assertEqual(
            'git://github.com/jelmer/dulwich',
            fixup_broken_git_url('git://github.com/jelmer/dulwich'))
        self.assertEqual(
            'https://github.com/jelmer/dulwich',
            fixup_broken_git_url('https://github.com/jelmer/dulwich'))

    def test_salsa_not_https(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/dulwich',
            fixup_broken_git_url(
                'git://salsa.debian.org/jelmer/dulwich'))

    def test_salsa_uses_cgit(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/dulwich',
            fixup_broken_git_url(
                'https://salsa.debian.org/cgit/jelmer/dulwich'))


class SanitizeUrlTests(TestCase):

    def test_simple(self):
        self.assertEqual(
            'http://github.com/jelmer/blah',
            sanitize_url('http://github.com/jelmer/blah'))

    def test_git_http(self):
        self.assertEqual(
            'http://github.com/jelmer/blah',
            sanitize_url('git+http://github.com/jelmer/blah'))
        self.assertEqual(
            'https://github.com/jelmer/blah',
            sanitize_url('git+https://github.com/jelmer/blah'))
