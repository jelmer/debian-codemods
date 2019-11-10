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

    def test_subpath(self):
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah/tree/debian/foo',
            determine_browser_url(
                'https://salsa.debian.org/js-team/node-blah -b debian [foo]'))
        self.assertEqual(
            'https://salsa.debian.org/js-team/node-blah/tree/HEAD/foo',
            determine_browser_url(
                'https://salsa.debian.org/js-team/node-blah [foo]'))


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
        self.assertEqual(
            'https://salsa.debian.org/perl-team/modules/packages/'
            'libgstream-interfaces-perl.git',
            salsa_url_from_alioth_url(
                'git',
                'git://git.debian.org/pkg-perl/packages/'
                'libgstream-interfaces-perl.git'))
        # TODO(jelmer): This should actually be
        # https://salsa.debian.org/qt-kde-team/extras/plasma-widget-menubar.git
        self.assertEqual(
            'https://salsa.debian.org/qt-kde-team/'
            'kde-extras/plasma-widget-menubar.git',
            salsa_url_from_alioth_url(
                'git',
                'git://anonscm.debian.org/pkg-kde/kde-extras/'
                'plasma-widget-menubar.git'))
        self.assertEqual(
            'https://salsa.debian.org/fonts-team/fonts-beteckna.git',
            salsa_url_from_alioth_url(
                'git',
                'git://anonscm.debian.org/pkg-fonts/fonts-beteckna.git'))
        self.assertEqual(
            'https://salsa.debian.org/brlink/xwit.git',
            salsa_url_from_alioth_url(
                'git',
                'git://anonscm.debian.org/users/brlink/xwit.git'))
        self.assertEqual(
            'https://salsa.debian.org/qt-kde-team/kde/kruler',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/git/pkg-kde/applications/'
                'kruler'))
        self.assertEqual(
            'https://salsa.debian.org/3dprinting-team/arduino-mighty',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/git/3dprinter/packages/'
                'arduino-mighty'))
        self.assertEqual(
            'https://salsa.debian.org/emacsen-team/lua-mode',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/git/pkg-emacsen/pkg/lua-mode'))
        self.assertEqual(
            'https://salsa.debian.org/debian-astro-team/astromatic.git',
            salsa_url_from_alioth_url(
                'git',
                'http://anonscm.debian.org/cgit/debian-astro/packages/'
                'astromatic.git'))
        self.assertEqual(
            'https://salsa.debian.org/debichem-team/bkchem.git',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/git/debichem/packages/bkchem.git'))
        self.assertEqual(
            'https://salsa.debian.org/3dprinting-team/yagv.git',
            salsa_url_from_alioth_url(
                'git',
                'https://anonscm.debian.org/cgit/3dprinter/packages/yagv.git'))

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
        self.assertEqual(
            'https://salsa.debian.org/python-team/applications/pypar2',
            salsa_url_from_alioth_url(
                'svn',
                'svn://anonscm.debian.org/python-apps/packages/pypar2/trunk/'
            ))
        self.assertEqual(
            'https://salsa.debian.org/xml-sgml-team/docbook-slides-demo',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/debian-xml-sgml/packages/'
                'docbook-slides-demo/trunk/'
            ))
        self.assertEqual(
            'https://salsa.debian.org/fonts-team/ttf-inconsolata',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/svn/pkg-fonts/packages/ttf-inconsolata'
            ))
        self.assertEqual(
            'https://salsa.debian.org/qt-kde-team/kde3libs',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/svn/pkg-kde/trunk/packages/kde3libs'
            ))
        self.assertEqual(
            'https://salsa.debian.org/python-team/applications/upnp-inspector',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/python-apps/packages/upnp-inspector/'
                'trunk/'
            ))
        self.assertEqual(
            'https://salsa.debian.org/python-team/applications/hotssh',
            salsa_url_from_alioth_url(
                'svn',
                'svn://anonscm.debian.org/python-apps/packages/hotssh/trunk/'
            ))
        self.assertEqual(
            'https://salsa.debian.org/debichem-team/drawxtl',
            salsa_url_from_alioth_url(
                'svn',
                'svn://svn.debian.org/svn/debichem/unstable/drawxtl'))
