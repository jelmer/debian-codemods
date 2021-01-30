#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij <jelmer@debian.org>
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

__all__ = [
    'plausible_url',
    'plausible_browse_url',
    'sanitize_url',
    'is_gitlab_site',
    'browse_url_from_repo_url',
    ]

from typing import Optional


from lintian_brush.vcs import (
    sanitize_url,
    is_gitlab_site,
    browse_url_from_repo_url,
    )


def plausible_browse_url(url: str) -> bool:
    return url.startswith('https://') or url.startswith('http://')


def plausible_url(url: str) -> bool:
    return ':' in url


def unsplit_vcs_url(repo_url: str,
                    branch: Optional[str] = None,
                    subpath: Optional[str] = None) -> str:
    """Unsplit a Debian VCS URL.

    Args:
      repo_url: Repository URL
      branch: Branch name
      subpath: Subpath in the tree
    Returns: full URL
    """
    url = repo_url
    if branch:
        url = '%s -b %s' % (url, branch)
    if subpath:
        url = '%s [%s]' % (url, subpath)
    return url
