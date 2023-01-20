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


import posixpath
import re
from typing import Optional, Union, List, Callable
from urllib.parse import urlparse, urlunparse

from debmutate.vcs import split_vcs_url, unsplit_vcs_url

from upstream_ontologist.vcs import (
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    find_secure_repo_url,
    canonical_git_repo_url,
    find_public_repo_url,
    fixup_rcp_style_git_repo_url,
    fixup_broken_git_details,
    is_gitlab_site,
)


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


def determine_gitlab_browser_url(url: str) -> str:
    (url, branch, subpath) = split_vcs_url(url)
    parsed_url = urlparse(url.rstrip("/"))
    # TODO(jelmer): Add support for branches
    path = parsed_url.path
    if path.endswith(".git"):
        path = path[: -len(".git")]
    if subpath and not branch:
        branch = "HEAD"
    if branch:
        path = path + "/tree/%s" % branch
    if subpath:
        path = path + "/" + subpath
    return f"https://{parsed_url.hostname}{path}"


def determine_browser_url(vcs_type, vcs_url: str) -> Optional[str]:
    repo_url, branch, subpath = split_vcs_url(vcs_url)
    parsed = urlparse(repo_url.rstrip("/"))
    if is_gitlab_site(parsed.netloc):
        return determine_gitlab_browser_url(vcs_url)
    if parsed.netloc == "github.com":
        path = parsed.path
        if path.endswith(".git"):
            path = path[:-4]
        if subpath and not branch:
            branch = "HEAD"
        if branch:
            path = posixpath.join(path, "tree", branch)
        if subpath:
            path = posixpath.join(path, subpath)
        return urlunparse(
            ("https", parsed.netloc, path, parsed.query,
             parsed.params, parsed.fragment)
        )
    if (
        parsed.netloc in ("code.launchpad.net", "launchpad.net")
        and not branch
        and not subpath
    ):
        return urlunparse(
            (
                "https",
                "code.launchpad.net",
                parsed.path,
                parsed.query,
                parsed.params,
                parsed.fragment,
            )
        )
    if parsed.hostname in ("git.savannah.gnu.org", "git.sv.gnu.org"):
        path_elements = parsed.path.strip("/").split("/")
        if parsed.scheme == "https" and path_elements[0] == "git":
            path_elements.pop(0)
        # Why cgit and not gitweb?
        path_elements.insert(0, "cgit")
        return urlunparse(
            ("https", parsed.netloc, "/".join(path_elements), None, None, None)
        )
    if parsed.hostname in ("git.code.sf.net", "git.code.sourceforge.net"):
        path_elements = parsed.path.strip("/").split("/")
        if path_elements[0] != "p":
            return None
        project = path_elements[1]
        repository = path_elements[2]
        path_elements = ["p", project, repository]
        if branch is not None:
            path_elements.extend(["ci", branch, "tree"])
        elif subpath is not None:
            path_elements.extend(["ci", "HEAD", "tree"])
        if subpath is not None:
            path_elements.append(subpath)
        return urlunparse(
            ("https", "sourceforge.net", "/".join(path_elements), None, None,
             None)
        )
    return None


def canonicalize_vcs_browser_url(url: str) -> str:
    url = url.replace(
        "https://svn.debian.org/wsvn/", "https://anonscm.debian.org/viewvc/"
    )
    url = url.replace(
        "http://svn.debian.org/wsvn/", "https://anonscm.debian.org/viewvc/"
    )
    url = url.replace(
        "https://git.debian.org/?p=", "https://anonscm.debian.org/git/")
    url = url.replace(
        "http://git.debian.org/?p=", "https://anonscm.debian.org/git/")
    url = url.replace(
        "https://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/"
    )
    url = url.replace(
        "http://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/"
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
        repo_url, branch=branch, net_access=net_access)
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
