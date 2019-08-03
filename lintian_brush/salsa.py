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


__all__ = ['guess_repository_url']

from urllib.parse import urlparse


MAINTAINER_EMAIL_MAP = {
    'pkg-javascript-devel@lists.alioth.debian.org': 'js-team',
    'python-modules-team@lists.alioth.debian.org': 'python-team/modules',
    'debian-science-maintainers@lists.alioth.debian.org': 'science-team',
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

    return 'https://salsa.debian.org/%s/%s' % (team_name, package)


def determine_browse_url(url):
    """Determine the browse URL from a regular Git URL.

    Args:
      url: Regular URL
    Returns:
      a browse URL
    """
    parsed_url = urlparse(url)
    # TODO(jelmer): Add support for branches
    assert parsed_url.netloc == 'salsa.debian.org'
    return 'https://salsa.debian.org%s' % parsed_url.path.rstrip('.git')
