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

"""Tests for lintian_brush.salsa."""

from breezy.tests import (
    TestCase,
    )
from lintian_brush.salsa import (
    guess_repository_url,
    determine_browser_url,
    salsa_url_from_alioth_url,
    )


class GuessRepositoryURLTests(TestCase):

    def test_unknown(self):
        self.assertIs(
            None,
            guess_repository_url(
                'blah', 'unknown-team@lists.alioth.debian.org'))

    def test_individual(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/lintian-brush.git',
            guess_repository_url('lintian-brush', 'jelmer@debian.org'))

    def test_team(self):
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah.git',
            guess_repository_url(
                'node-blah', 'pkg-javascript-devel@lists.alioth.debian.org'))


class DetermineBrowserUrlTests(TestCase):

    def test_browser_url(self):
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah',
            determine_browser_url(
                'https://salsa.debian.org/js-team/node-blah.git'))
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah',
            determine_browser_url(
                'https://salsa.debian.org/js-team/node-blah'))

    def test_branch(self):
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah/tree/debian',
            determine_browser_url(
                'https://salsa.debian.org/js-team/node-blah -b debian'))


class SalsaUrlFromAliothUrlTests(TestCase):

    def test_none(self):
        self.assertIs(None, salsa_url_from_alioth_url(None, None))

    def test_mismatch(self):
        self.assertIs(
            None, salsa_url_from_alioth_url(
                'bzr', 'https://code.launchpad.net/blah'))

    def test_perl(self):
        self.assertEqual(
            'https://salsa.debian.org/perl-team/modules/packages/libbla',
            salsa_url_from_alioth_url(
                'svn', 'svn://svn.debian.org/pkg-perl/trunk/libbla'))

    def test_git(self):
        self.assertEqual(
            'https://salsa.debian.org/jelmer/python-bla',
            salsa_url_from_alioth_url(
                'git',
                'http://anonscm.debian.org/git/users/jelmer/python-bla')
            )
        self.assertEqual(
            'https://salsa.debian.org/jelmer/python-bla',
            salsa_url_from_alioth_url(
                'git',
                'http://anonscm.debian.org/users/jelmer/python-bla')
            )
        self.assertEqual(
            'https://salsa.debian.org/go-team/golang-example-blah-blah',
            salsa_url_from_alioth_url(
                'git',
                'http://anonscm.debian.org/pkg-go/golang-example-blah-blah')
            )
        self.assertEqual(
            'https://salsa.debian.org/lua-team/blah',
            salsa_url_from_alioth_url(
                'git',
                'https://alioth.debian.org/anonscm/git/pkg-lua/blah'))
        self.assertEqual(
            'https://salsa.debian.org/science-team/ros-geometry.git',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/cgit/debian-science/'
                'ros-geometry.git'))
        self.assertEqual(
            'https://salsa.debian.org/nagios-team/pkg-check-multi.git',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/git/pkg-nagios/'
                'pkg-check-multi.git'))

    def test_svn(self):
        self.assertEqual(
            'https://salsa.debian.org/multimedia-team/ezstream',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/svn/pkg-icecast/ezstream/trunk')
            )
        self.assertEqual(
            'https://salsa.debian.org/bsd-team/freebsd-buildutils',
            salsa_url_from_alioth_url(
                'svn',
                'svn://anonscm.debian.org/glibc-bsd/trunk/freebsd-buildutils/')
            )
        self.assertEqual(
            'https://salsa.debian.org/nvidia-team/nvclock',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/pkg-nvidia/packages/nvclock/trunk'
            ))
        self.assertEqual(
            'https://salsa.debian.org/llvm-team/llvm',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/svn/pkg-llvm/llvm/trunk/'
            ))
        self.assertEqual(
            'https://salsa.debian.org/xfce-team/xfswitch-plugin',
            salsa_url_from_alioth_url(
                'svn',
                'svn://anonscm.debian.org/pkg-xfce/goodies/trunk/'
                'xfswitch-plugin/'
            ))
