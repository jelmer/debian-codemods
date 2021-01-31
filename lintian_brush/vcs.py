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
    'fixup_broken_git_url',
    'sanitize_url',
    'determine_browser_url',
    ]


import posixpath
import re
import socket
from typing import Optional, Union, List
import urllib.error
from urllib.parse import urlparse, urlunparse, ParseResult
from urllib.request import urlopen, Request

from debmutate.vcs import split_vcs_url, unsplit_vcs_url

from upstream_ontologist.vcs import (
    drop_vcs_in_scheme,
    find_secure_repo_url,
    canonical_git_repo_url,
    find_public_repo_url,
    fixup_rcp_style_git_repo_url,
    )

from lintian_brush import (
    USER_AGENT,
    DEFAULT_URLLIB_TIMEOUT,
    )


KNOWN_GITLAB_SITES = [
    'salsa.debian.org',
    'invent.kde.org',
    ]


def probe_gitlab_host(hostname: str):
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    try:
        urlopen(
            Request('https://%s/api/v4/version' % hostname, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.status == 401:
            import json
            if json.loads(e.read()) == {"message": "401 Unauthorized"}:
                return True
    except (socket.timeout, urllib.error.URLError):
        # Probably not?
        return False
    return False


def is_gitlab_site(hostname: str, net_access: bool = False) -> bool:
    if hostname is None:
        return False
    if hostname in KNOWN_GITLAB_SITES:
        return True
    if hostname.startswith('gitlab.'):
        return True
    if net_access:
        return probe_gitlab_host(hostname)
    return False


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


def convert_cvs_list_to_str(urls):
    if not isinstance(urls, list):
        return urls
    if urls[0].startswith(':extssh:') or urls[0].startswith(':pserver:'):
        try:
            from breezy.location import cvs_to_url
        except ImportError:
            from breezy.location import pserver_to_url as cvs_to_url
            if urls[0].startswith(':extssh:'):
                raise NotImplementedError(
                    'unable to deal with extssh CVS locations.')
        return cvs_to_url(urls[0]) + '#' + urls[1]
    return urls


def fix_path_in_port(parsed: ParseResult, branch: Optional[str]):
    if ':' not in parsed.netloc or parsed.netloc.endswith(']'):
        return None, None
    host, port = parsed.netloc.rsplit(':', 1)
    if host.split('@')[-1] not in (KNOWN_GITLAB_SITES + ['github.com']):
        return None, None
    if not port or port.isdigit():
        return None, None
    return parsed._replace(
        path='%s/%s' % (port, parsed.path.lstrip('/')),
        netloc=host), branch


def fix_gitlab_scheme(parsed, branch):
    if is_gitlab_site(parsed.hostname):
        return parsed._replace(scheme='https'), branch
    return None, None


def fix_salsa_cgit_url(parsed, branch):
    if (parsed.hostname == 'salsa.debian.org' and
            parsed.path.startswith('/cgit/')):
        return parsed._replace(path=parsed.path[5:]), branch
    return None, None


def fix_gitlab_tree_in_url(parsed, branch):
    if is_gitlab_site(parsed.hostname):
        parts = parsed.path.split('/')
        if len(parts) >= 5 and parts[3] == 'tree':
            branch = '/'.join(parts[4:])
            return parsed._replace(path='/'.join(parts[:3])), branch
    return None, None


def fix_double_slash(parsed, branch):
    if parsed.path.startswith('//'):
        return parsed._replace(path=parsed.path[1:]), branch
    return None, None


def fix_extra_colon(parsed, branch):
    return parsed._replace(netloc=parsed.netloc.rstrip(':')), branch


def drop_git_username(parsed, branch):
    if parsed.hostname not in ('salsa.debian.org', 'github.com'):
        return None, None
    if parsed.scheme not in ('git', 'http', 'https'):
        return None, None
    if parsed.username == 'git' and parsed.netloc.startswith('git@'):
        return parsed._replace(netloc=parsed.netloc[4:]), branch
    return None, None


def fix_branch_argument(parsed, branch):
    if parsed.hostname != 'github.com':
        return None, None
    # TODO(jelmer): Handle gitlab sites too?
    path_elements = parsed.path.strip('/').split('/')
    if len(path_elements) > 2 and path_elements[2] == 'tree':
        return (parsed._replace(path='/'.join(path_elements[:2])),
                '/'.join(path_elements[3:]))
    return None, None


def fix_git_gnome_org_url(parsed, branch):
    if parsed.netloc == 'git.gnome.org':
        if parsed.path.startswith('/browse'):
            path = parsed.path[7:]
        else:
            path = parsed.path
        parsed = parsed._replace(
            netloc='gitlab.gnome.org', scheme='https',
            path='/GNOME' + path)
        return parsed, branch
    return None, None


def fix_anongit_url(parsed, branch):
    if parsed.netloc == 'anongit.kde.org' and parsed.scheme == 'git':
        parsed = parsed._replace(scheme='https')
        return parsed, branch
    return None, None


def fix_freedesktop_org_url(parsed: ParseResult, branch: Optional[str]):
    if parsed.netloc == 'anongit.freedesktop.org':
        path = parsed.path
        if path.startswith('/git/'):
            path = path[len('/git'):]
        parsed = parsed._replace(
            netloc='gitlab.freedesktop.org', scheme='https',
            path=path)
        return parsed, branch
    return None, None


FIXERS = [
    fix_path_in_port,
    fix_gitlab_scheme,
    fix_salsa_cgit_url,
    fix_gitlab_tree_in_url,
    fix_double_slash,
    fix_extra_colon,
    drop_git_username,
    fix_branch_argument,
    fix_git_gnome_org_url,
    fix_anongit_url,
    fix_freedesktop_org_url,
    ]


def fixup_broken_git_url(url: str) -> str:
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    repo_url, branch, subpath = split_vcs_url(url)

    parsed = urlparse(repo_url)
    changed = False
    for fn in FIXERS:
        newparsed, newbranch = fn(parsed, branch)
        if newparsed:
            changed = True
            parsed = newparsed
            branch = newbranch

    if changed:
        return unsplit_vcs_url(urlunparse(parsed), branch, subpath)
    return url


def determine_gitlab_browser_url(url: str) -> str:
    (url, branch, subpath) = split_vcs_url(url)
    parsed_url = urlparse(url.rstrip('/'))
    # TODO(jelmer): Add support for branches
    path = parsed_url.path
    if path.endswith('.git'):
        path = path[:-len('.git')]
    if subpath and not branch:
        branch = "HEAD"
    if branch:
        path = path + '/tree/%s' % branch
    if subpath:
        path = path + '/' + subpath
    return 'https://%s%s' % (parsed_url.hostname, path)


def determine_browser_url(vcs_type, vcs_url: str) -> Optional[str]:
    repo_url, branch, subpath = split_vcs_url(vcs_url)
    parsed = urlparse(repo_url.rstrip('/'))
    if is_gitlab_site(parsed.netloc):
        return determine_gitlab_browser_url(vcs_url)
    if parsed.netloc == 'github.com':
        path = parsed.path
        if path.endswith('.git'):
            path = path[:-4]
        if subpath and not branch:
            branch = "HEAD"
        if branch:
            path = posixpath.join(path, 'tree', branch)
        if subpath:
            path = posixpath.join(path, subpath)
        return urlunparse(
            ('https', parsed.netloc, path,
             parsed.query, parsed.params, parsed.fragment))
    if (parsed.netloc in ('code.launchpad.net', 'launchpad.net') and
            not branch and not subpath):
        return urlunparse(
            ('https', 'code.launchpad.net', parsed.path,
             parsed.query, parsed.params, parsed.fragment))
    if parsed.hostname in ('git.savannah.gnu.org', 'git.sv.gnu.org'):
        path_elements = parsed.path.strip('/').split('/')
        if parsed.scheme == 'https' and path_elements[0] == 'git':
            path_elements.pop(0)
        # Why cgit and not gitweb?
        path_elements.insert(0, 'cgit')
        return urlunparse(
            ('https', parsed.netloc, '/'.join(path_elements), None,
             None, None))
    if parsed.hostname in ('git.code.sf.net', 'git.code.sourceforge.net'):
        path_elements = parsed.path.strip('/').split('/')
        if path_elements[0] != 'p':
            return None
        project = path_elements[1]
        repository = path_elements[2]
        path_elements = ['p', project, repository]
        if branch is not None:
            path_elements.extend(['ci', branch, 'tree'])
        elif subpath is not None:
            path_elements.extend(['ci', 'HEAD', 'tree'])
        if subpath is not None:
            path_elements.append(subpath)
        return urlunparse(
            ('https', 'sourceforge.net', '/'.join(path_elements), None, None,
             None))
    return None


def canonicalize_vcs_browser_url(url: str) -> str:
    url = url.replace(
        "https://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/")
    url = url.replace(
        "http://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/")
    url = url.replace(
        "https://git.debian.org/?p=",
        "https://anonscm.debian.org/git/")
    url = url.replace(
        "http://git.debian.org/?p=",
        "https://anonscm.debian.org/git/")
    url = url.replace(
        "https://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/")
    url = url.replace(
        "http://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/")
    url = re.sub(
        r"^https?://salsa.debian.org/([^/]+/[^/]+)\.git/?$",
        "https://salsa.debian.org/\\1",
        url)
    return url


def canonical_vcs_git_url(url: str) -> str:
    (repo_url, branch, subpath) = split_vcs_url(url)
    repo_url = canonical_git_repo_url(repo_url)
    return unsplit_vcs_url(repo_url, branch, subpath)


canonicalize_vcs_fns = {
    'Browser': canonicalize_vcs_browser_url,
    'Git': canonical_vcs_git_url,
}


def canonicalize_vcs_url(vcs_type: str, url: str) -> str:
    try:
        return canonicalize_vcs_fns[vcs_type](url)
    except KeyError:
        return url


def find_secure_vcs_url(url: str, net_access: bool = True) -> Optional[str]:
    (repo_url, branch, subpath) = split_vcs_url(url)
    repo_url = find_secure_repo_url(
        repo_url, branch=branch, net_access=net_access)
    if repo_url is None:
        return None

    return unsplit_vcs_url(repo_url, branch, subpath)


SANITIZERS = [
    convert_cvs_list_to_str,
    drop_vcs_in_scheme,
    fixup_broken_git_url,
    fixup_rcp_style_git_url,
    lambda url: find_public_vcs_url(url) or url,
    canonical_vcs_git_url,
    lambda url: find_secure_vcs_url(url, net_access=False) or url,
]


def sanitize_url(url: Union[str, List[str]]) -> str:
    if isinstance(url, str):
        url = url.strip()
    for sanitizer in SANITIZERS:
        url = sanitizer(url)
    return url
