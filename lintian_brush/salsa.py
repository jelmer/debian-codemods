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
    if ' -b ' in url:
        (url, branch) = url.split(' -b ', 1)
    else:
        branch = None
    parsed_url = urlparse(url)
    # TODO(jelmer): Add support for branches
    assert parsed_url.netloc == 'salsa.debian.org'
    path = parsed_url.path
    if path.endswith('.git'):
        path = path[:-len('.git')]
    if branch:
        path = path + '/tree/%s' % branch
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
        m = "(https?|git)://(anonscm|git).debian.org/(git/)?collab-maint/"
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/debian/', alioth_url)
        m = "(https?|git)://(anonscm|git).debian.org/(git/)?pkg-lua/"
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/lua-team/', alioth_url)
        m = "(https?|git)://(anonscm|git).debian.org/(git/)?users/"
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/', alioth_url)
        m = "(https?|git)://(anonscm|git).debian.org/(git/)?"
        if re.match(m, alioth_url):
            return re.sub(m, 'https://salsa.debian.org/', alioth_url)

    if vcs_type.lower() == 'svn':
        if alioth_url.startswith('svn://svn.debian.org/pkg-perl/trunk'):
            return alioth_url.replace(
                'svn://svn.debian.org/pkg-perl/trunk',
                'https://salsa.debian.org/perl-team/modules/packages')
        if alioth_url.startswith('svn://svn.debian.org/pkg-lua/packages'):
            return alioth_url.replace(
                'svn://svn.debian.org/pkg-lua/packages',
                'https://salsa.debian.org/lua-team')
    return None
