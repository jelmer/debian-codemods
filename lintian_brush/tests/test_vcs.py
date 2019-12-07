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
    canonicalize_vcs_url,
    determine_browser_url,
    fixup_broken_git_url,
    plausible_url,
    sanitize_url,
    split_vcs_url,
    unsplit_vcs_url,
    )


class FixUpGitUrlTests(TestCase):

    def test_fixup(self):
        self.assertEqual(
            'git://github.com/jelmer/dulwich',
            fixup_broken_git_url('git://github.com:jelmer/dulwich'))
        self.assertEqual(
            'git://github.com/jelmer/dulwich -b blah',
            fixup_broken_git_url('git://github.com:jelmer/dulwich -b blah'))

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

    def test_salsa_tree_branch(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/dulwich -b master',
            fixup_broken_git_url(
                'https://salsa.debian.org/jelmer/dulwich/tree/master'))

    def test_strip_extra_slash(self):
        self.assertEqual(
            'https://salsa.debian.org/salve/auctex.git',
            fixup_broken_git_url('https://salsa.debian.org//salve/auctex.git'))

    def test_strip_extra_colon(self):
        self.assertEqual(
            'https://salsa.debian.org/mckinstry/lcov.git',
            fixup_broken_git_url(
                'https://salsa.debian.org:/mckinstry/lcov.git'))

    def test_strip_username(self):
        self.assertEqual(
            'git://github.com/RPi-Distro/pgzero.git',
            fixup_broken_git_url('git://git@github.com:RPi-Distro/pgzero.git'))
        self.assertEqual(
            'https://salsa.debian.org/debian-astro-team/pyavm.git',
            fixup_broken_git_url(
                'https://git@salsa.debian.org:debian-astro-team/pyavm.git'))

    def test_github_tree_url(self):
        self.assertEqual(
            'https://github.com/blah/blah -b master',
            fixup_broken_git_url('https://github.com/blah/blah/tree/master'))

    def test_freedesktop(self):
        self.assertEqual(
            'https://gitlab.freedesktop.org/xorg/xserver',
            fixup_broken_git_url('git://anongit.freedesktop.org/xorg/xserver'))

    def test_gnome(self):
        self.assertEqual(
            'https://gitlab.gnome.org/GNOME/alacarte',
            fixup_broken_git_url('https://git.gnome.org/browse/alacarte'))


class SanitizeUrlTests(TestCase):

    def test_simple(self):
        self.assertEqual(
            'https://github.com/jelmer/blah.git',
            sanitize_url('http://github.com/jelmer/blah'))

    def test_git_http(self):
        self.assertEqual(
            'https://github.com/jelmer/blah.git',
            sanitize_url('git+http://github.com/jelmer/blah'))
        self.assertEqual(
            'https://github.com/jelmer/blah.git',
            sanitize_url('git+https://github.com/jelmer/blah'))


class DetermineBrowserUrlTests(TestCase):

    def test_salsa(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/dulwich',
            determine_browser_url(
                'git', 'https://salsa.debian.org/jelmer/dulwich.git'))

    def test_github(self):
        self.assertEqual(
            'https://github.com/jelmer/dulwich',
            determine_browser_url(
                'git', 'https://github.com/jelmer/dulwich.git'))
        self.assertEqual(
            'https://github.com/jelmer/dulwich/tree/master',
            determine_browser_url(
                'git', 'https://github.com/jelmer/dulwich.git -b master'))
        self.assertEqual(
            'https://github.com/jelmer/dulwich/tree/master',
            determine_browser_url(
                'git', 'git://github.com/jelmer/dulwich -b master'))
        self.assertEqual(
            'https://github.com/jelmer/dulwich/tree/master/blah',
            determine_browser_url(
                'git', 'git://github.com/jelmer/dulwich -b master [blah]'))
        self.assertEqual(
            'https://github.com/jelmer/dulwich/tree/HEAD/blah',
            determine_browser_url(
                'git', 'git://github.com/jelmer/dulwich [blah]'))


class PlausibleUrlTests(TestCase):

    def test_url(self):
        self.assertFalse(plausible_url('the'))
        self.assertFalse(plausible_url('1'))
        self.assertTrue(plausible_url('git@foo:blah'))
        self.assertTrue(plausible_url('git+ssh://git@foo/blah'))
        self.assertTrue(plausible_url('https://foo/blah'))


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


class CanonicalizeVcsUrlTests(TestCase):

    def test_github(self):
        self.assertEqual(
            'https://github.com/jelmer/example.git',
            canonicalize_vcs_url('Git', 'https://github.com/jelmer/example'))

    def test_salsa(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/example.git',
            canonicalize_vcs_url(
                'Git', 'https://salsa.debian.org/jelmer/example'))
        self.assertEqual(
            'https://salsa.debian.org/jelmer/example.git',
            canonicalize_vcs_url(
                'Git', 'https://salsa.debian.org/jelmer/example.git'))
