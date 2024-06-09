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
    "sanitize_url",
    "determine_browser_url",
]


import re
from typing import Callable, List, Optional, Union

from debmutate.vcs import split_vcs_url, unsplit_vcs_url
from upstream_ontologist.vcs import (
    canonical_git_repo_url,
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    find_public_repo_url,
    find_secure_repo_url,
    fixup_broken_git_details,
    fixup_rcp_style_git_repo_url,
)

from . import _lintian_brush_rs


def find_public_vcs_url(url: str) -> Optional[str]:
    (repo_url, branch, subpath) = split_vcs_url(url)
    revised_url = find_public_repo_url(repo_url)
    if revised_url is not None:
        return unsplit_vcs_url(revised_url, branch, subpath)
    return None


def fixup_rcp_style_git_url(url: str) -> str:
    (repo_url, branch, subpath) = split_vcs_url(url)
    repo_url = fixup_rcp_style_git_repo_url(repo_url)
    return unsplit_vcs_url(repo_url, branch, subpath)


determine_gitlab_browser_url = _lintian_brush_rs.determine_gitlab_browser_url
determine_browser_url = _lintian_brush_rs.determine_browser_url


def canonicalize_vcs_browser_url(url: str) -> str:
    url = url.replace(
        "https://svn.debian.org/wsvn/", "https://anonscm.debian.org/viewvc/"
    )
    url = url.replace(
        "http://svn.debian.org/wsvn/", "https://anonscm.debian.org/viewvc/"
    )
    url = url.replace(
        "https://git.debian.org/?p=", "https://anonscm.debian.org/git/"
    )
    url = url.replace(
        "http://git.debian.org/?p=", "https://anonscm.debian.org/git/"
    )
    url = url.replace(
        "https://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/",
    )
    url = url.replace(
        "http://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/",
    )
    return re.sub(
        r"^https?://salsa.debian.org/([^/]+/[^/]+)\.git/?$",
        "https://salsa.debian.org/\\1",
        url,
    )


def canonical_vcs_git_url(url: str) -> str:
    (repo_url, branch, subpath) = split_vcs_url(url)
    repo_url = canonical_git_repo_url(repo_url)
    return unsplit_vcs_url(repo_url, branch, subpath)


canonicalize_vcs_fns = {
    "Browser": canonicalize_vcs_browser_url,
    "Git": canonical_vcs_git_url,
}


def canonicalize_vcs_url(vcs_type: str, url: str) -> str:
    try:
        return canonicalize_vcs_fns[vcs_type](url)
    except KeyError:
        return url


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


SANITIZERS: List[Callable[[str], str]] = [
    drop_vcs_in_scheme,
    fixup_broken_git_url,
    fixup_rcp_style_git_url,
    lambda url: find_public_vcs_url(url) or url,
    canonical_vcs_git_url,
    lambda url: find_secure_vcs_url(url, net_access=False) or url,
]


def sanitize_url(url: Union[str, List[str]]) -> str:
    """Sanitize a version control URL."""
    if isinstance(url, list):
        url_str = convert_cvs_list_to_str(url)
    else:
        url_str = url
    url_str = url_str.strip()
    for sanitizer in SANITIZERS:
        url_str = sanitizer(url_str)
    return url_str
