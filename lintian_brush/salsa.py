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

"""Utility functions for dealing with control files."""

__all__ = ['guess_repository_url', 'determine_browser_url']

import re
from urllib.parse import urlparse

from .vcs import split_vcs_url


MAINTAINER_EMAIL_MAP = {
    'pkg-javascript-devel@lists.alioth.debian.org': 'js-team',
    'python-modules-team@lists.alioth.debian.org': 'python-team/modules',
    'python-apps-team@lists.alioth.debian.org': 'python-team/applications',
    'debian-science-maintainers@lists.alioth.debian.org': 'science-team',
    'pkg-perl-maintainers@lists.alioth.debian.org':
        'perl-team/modules/packages',
    'pkg-java-maintainers@lists.alioth.debian.org': 'java-team',
    'pkg-ruby-extras-maintainers@lists.alioth.debian.org': 'ruby-team',
    'pkg-clamav-devel@lists.alioth.debian.org': 'clamav-team',
    'pkg-go-maintainers@lists.alioth.debian.org': 'go-team/packages',
    'pkg-games-devel@lists.alioth.debian.org': 'games-team',
    'pkg-telepathy-maintainers@lists.alioth.debian.org': 'telepathy-team',
    'debian-fonts@lists.debian.org': 'fonts-team',
    'pkg-gnustep-maintainers@lists.alioth.debian.org': 'gnustep-team',
    'pkg-gnome-maintainers@lists.alioth.debian.org': 'gnome-team',
    'pkg-multimedia-maintainers@lists.alioth.debian.org': 'multimedia-team',
    'debian-ocaml-maint@lists.debian.org': 'ocaml-team',
    'pkg-php-pear@lists.alioth.debian.org': 'php-team/pear',
    'pkg-mpd-maintainers@lists.alioth.debian.org': 'mpd-team',
    'pkg-cli-apps-team@lists.alioth.debian.org': 'dotnet-team',
    'pkg-mono-group@lists.alioth.debian.org': 'dotnet-team',
    }

TEAM_NAME_MAP = {
    'debian-xml-sgml': 'xml-sgml-team',
    'pkg-go': 'go-team',
    'pkg-fonts': 'fonts-team',
    'pkg-javascript': 'js-team',
    'pkg-java': 'java-team',
    'pkg-mpd': 'mpd-team',
    'pkg-electronics': 'electronics-team',
    'pkg-xfce': 'xfce-team',
    'pkg-lxc': 'lxc-team',
    'debian-science': 'science-team',
    'pkg-games': 'games-team',
    'pkg-bluetooth': 'bluetooth-team',
    'debichem': 'debichem-team',
    'openstack': 'openstack-team',
    'pkg-kde': 'qt-kde-team',
    'debian-islamic': 'islamic-team',
    'pkg-lua': 'lua-team',
    'pkg-xorg': 'xorg-team',
    'debian-astro': 'debian-astro-team',
    'pkg-icecast': 'multimedia-team',
    'glibc-bsd': 'bsd-team',
    'pkg-nvidia': 'nvidia-team',
    'pkg-llvm': 'llvm-team',
    'pkg-nagios': 'nagios-team',
    'pkg-sugar': 'pkg-sugar-team',
    'pkg-phototools': 'debian-phototools-team',
    'pkg-netmeasure': 'ineteng-team',
    'pkg-hamradio': 'debian-hamradio-team',
    'pkg-sass': 'sass-team',
    'pkg-rpm': 'pkg-rpm-team',
    'tts': 'tts-team',
    'python-apps': 'python-team/applications',
    'pkg-monitoring': 'monitoring-team',
    'pkg-perl': 'perl-team/modules',
    'debian-iot': 'debian-iot-team',
    'pkg-bitcoin': 'cryptocoin-team',
    'pkg-cyrus-imapd': 'debian',
    'pkg-dns': 'dns-team',
    'pkg-freeipa': 'freeipa-team',
    'pkg-ocaml-team': 'ocaml-team',
    'pkg-vdr-dvb': 'vdr-team',
    'debian-in': 'debian-in-team',
    'pkg-octave': 'pkg-octave-team',
    'pkg-postgresql': 'postgresql',
    'pkg-grass': 'debian-gis-team',
    'pkg-evolution': 'gnome-team',
    'pkg-gnome': 'gnome-team',
    'pkg-exppsy': 'neurodebian-team',
    'pkg-voip': 'pkg-voip-team',
    'pkg-privacy': 'pkg-privacy-team',
    'pkg-libvirt': 'libvirt-team',
    'debian-ha': 'ha-team',
    'debian-lego': 'debian-lego-team',
    'calendarserver': 'calendarserver-team',
    '3dprinter': '3dprinting-team',
    'pkg-multimedia': 'multimedia-team',
    'pkg-emacsen': 'emacsen-team',
    'pkg-haskell': 'haskell-team',
    'pkg-gnutls': 'gnutls-team',
    'pkg-mysql': 'mariadb-team',
    'pkg-php': 'php-team',
    'pkg-qemu': 'qemu-team',
    'pkg-xmpp': 'xmpp-team',
    'uefi': 'efi-team',
    'pkg-manpages-fr': 'l10n-fr-team',
    'pkg-proftpd': 'debian-proftpd-team',
    'pkg-apache': 'apache-team',
}

GIT_PATH_RENAMES = {
    'pkg-kde/applications': 'qt-kde-team/kde',
    '3dprinter/packages': '3dprinting-team',
    'pkg-emacsen/pkg': 'emacsen-team',
    'debian-astro/packages': 'debian-astro-team',
    'debian-islamic/packages': 'islamic-team',
    'debichem/packages': 'debichem-team',
    'pkg-privacy/packages': 'pkg-privacy-team',
}


def guess_repository_url(package, maintainer_email):
    """Guess the repository URL for a package hosted on Salsa.

    Args:
      package: Package name
      maintainer_email: The maintainer's email address (e.g. team list address)
    Returns:
      A guessed repository URL
    """
    if maintainer_email.endswith('@debian.org'):
        team_name = maintainer_email.split('@')[0]
    else:
        try:
            team_name = MAINTAINER_EMAIL_MAP[maintainer_email]
        except KeyError:
            return None

    return 'https://salsa.debian.org/%s/%s.git' % (team_name, package)


def determine_browser_url(url):
    """Determine the browser URL from a regular Git URL.

    Args:
      url: Regular URL
    Returns:
      a browser URL
    """
    (url, branch, subpath) = split_vcs_url(url)
    parsed_url = urlparse(url)
    # TODO(jelmer): Add support for branches
    assert parsed_url.netloc == 'salsa.debian.org'
    path = parsed_url.path
    if path.endswith('.git'):
        path = path[:-len('.git')]
    if subpath and not branch:
        branch = "HEAD"
    if branch:
        path = path + '/tree/%s' % branch
    if subpath:
        path = path + '/' + subpath
    return 'https://salsa.debian.org%s' % path


def salsa_url_from_alioth_url(vcs_type, alioth_url):
    """Guess the salsa URL from an alioth URL.

    Args:
      vcs_type: VCS type
      alioth_url: Alioth URL
    Returns:
      Salsa URL
    """
    if vcs_type is None:
        return None
    # These two regular expressions come from vcswatch:
    # https://salsa.debian.org/qa/qa/blob/master/data/vcswatch/vcswatch#L165
    if vcs_type.lower() == 'git':
        m = ("(https?|git)://(anonscm|git).debian.org/"
             "(cgit/|git/)?collab-maint/")
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/debian/', alioth_url)
        m = "(https?|git)://(anonscm|git).debian.org/(cgit/|git/)?users/"
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/', alioth_url)
        m = re.match(
            "(https?|git)://(anonscm|git).debian.org/(cgit/|git/)?(.+)",
            alioth_url)
        if m:
            parts = m.group(4).split('/')
            for i in range(len(parts), 0, -1):
                subpath = '/'.join(parts[:i])
                try:
                    return (
                        'https://salsa.debian.org/' +
                        GIT_PATH_RENAMES[subpath] + '/' +
                        '/'.join(parts[i:]))
                except KeyError:
                    pass
        m = re.match(
            "(https?|git)://(anonscm|git).debian.org/(cgit/|git/)?([^/]+)/",
            alioth_url)
        if m and m.group(4) in TEAM_NAME_MAP:
            new_name = TEAM_NAME_MAP[m.group(4)]
            return re.sub(m.re, 'https://salsa.debian.org/' + new_name + '/',
                          alioth_url)
        m = re.match(
            'https?://alioth.debian.org/anonscm/(git/|cgit/)?([^/]+)/',
            alioth_url)
        if m and m.group(2) in TEAM_NAME_MAP:
            new_name = TEAM_NAME_MAP[m.group(2)]
            return re.sub(m.re, 'https://salsa.debian.org/' + new_name + '/',
                          alioth_url)

    if vcs_type.lower() == 'svn':
        if alioth_url.startswith('svn://svn.debian.org/pkg-perl/trunk'):
            return alioth_url.replace(
                'svn://svn.debian.org/pkg-perl/trunk',
                'https://salsa.debian.org/perl-team/modules/packages')
        if alioth_url.startswith('svn://svn.debian.org/pkg-lua/packages'):
            return alioth_url.replace(
                'svn://svn.debian.org/pkg-lua/packages',
                'https://salsa.debian.org/lua-team')
        parsed_url = urlparse(alioth_url)
        if (parsed_url.scheme == 'svn' and
                parsed_url.netloc in (
                    ('svn.debian.org', 'anonscm.debian.org'))):
            parts = parsed_url.path.strip('/').split('/')
            if parts[0] == 'svn':
                parts.pop(0)
            if (len(parts) == 3 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[2] == 'trunk'):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[1])
            if (len(parts) == 3 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[1] == 'trunk'):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[2])
            if (len(parts) == 4 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[1] == 'packages' and
                    parts[3] == 'trunk'):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[2])
            if (len(parts) == 4 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[1] == 'trunk' and parts[2] == 'packages'):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[3])
            if (len(parts) > 3 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[-2] == 'trunk'):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[-1])
            if (len(parts) == 3 and
                    parts[0] in TEAM_NAME_MAP and
                    parts[1] in ('packages', 'unstable')):
                return 'https://salsa.debian.org/%s/%s' % (
                    TEAM_NAME_MAP[parts[0]], parts[2])
    return None
