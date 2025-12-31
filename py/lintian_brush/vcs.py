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

"""Utility functions for dealing with Vcs URLs of various types."""

__all__ = [
    "fixup_broken_git_url",
    "determine_browser_url",
]


from typing import Optional

from debmutate.vcs import split_vcs_url, unsplit_vcs_url
from upstream_ontologist.vcs import (
    find_secure_repo_url,
    fixup_broken_git_details,
)

from . import _lintian_brush_rs

determine_browser_url = _lintian_brush_rs.determine_browser_url


def find_secure_vcs_url(url: str, net_access: bool = True) -> Optional[str]:
    repo_url: Optional[str]
    (repo_url, branch, subpath) = split_vcs_url(url)
    repo_url = find_secure_repo_url(
        repo_url, branch=branch, net_access=net_access
    )
    if repo_url is None:
        return None

    return unsplit_vcs_url(repo_url, branch, subpath)


def fixup_broken_git_url(url: str) -> str:
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    repo_url, branch, subpath = split_vcs_url(url)
    newrepo_url, newbranch, newsubpath = fixup_broken_git_details(
        repo_url, branch, subpath
    )
    if newrepo_url != repo_url or newbranch != branch or newsubpath != subpath:
        return unsplit_vcs_url(newrepo_url, newbranch, newsubpath)
    return url
