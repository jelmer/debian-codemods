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

"""Utility functions for dealing with Debian Vcs URLs of various types."""

import re
from typing import Optional, Tuple


def split_vcs_url(url: str) -> Tuple[str, Optional[str], Optional[str]]:
    subpath: Optional[str]
    branch: Optional[str]
    m = re.search(r' \[([^] ]+)\]', url)
    if m:
        url = url[:m.start()] + url[m.end():]
        subpath = m.group(1)
    else:
        subpath = None
    try:
        (repo_url, branch) = url.split(' -b ', 1)
    except ValueError:
        branch = None
        repo_url = url
    return (repo_url, branch, subpath)


def unsplit_vcs_url(repo_url: str,
                    branch: Optional[str] = None,
                    subpath: Optional[str] = None) -> str:
    url = repo_url
    if branch:
        url = '%s -b %s' % (url, branch)
    if subpath:
        url = '%s [%s]' % (url, subpath)
    return url


def get_vcs_info(control) -> Tuple[
        Optional[str], Optional[str], Optional[str]]:
    if "Vcs-Git" in control:
        repo_url, branch, subpath = split_vcs_url(control["Vcs-Git"])
        return ("Git", repo_url, subpath)

    if "Vcs-Bzr" in control:
        return ("Bzr", control["Vcs-Bzr"], None)

    if "Vcs-Svn" in control:
        return ("Svn", control["Vcs-Svn"], None)

    if "Vcs-Hg" in control:
        repo_url, branch, subpath = split_vcs_url(control["Vcs-Hg"])
        return ("Hg", repo_url, subpath)

    return None, None, None
