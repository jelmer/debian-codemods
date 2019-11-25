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
    'extract_vcs_url_branch',
    'split_vcs_url',
    'determine_browser_url',
    ]


import posixpath
import re
from urllib.parse import urlparse, urlunparse


KNOWN_GITLAB_SITES = ['gitlab.com', 'salsa.debian.org', 'gitlab.gnome.org']


def extract_vcs_url_branch(url):
    # Deprecated, use split_vcs_url
    (repo_url, branch, subpath) = split_vcs_url(url)
    if subpath:
        repo_url = '%s [%s]' % (repo_url, subpath)
    return repo_url, branch


def split_vcs_url(url):
    m = re.finditer(r' \[([^] ]+)\]', url)
    try:
        m = next(m)
        url = url[:m.start()] + url[m.end():]
        subpath = m.group(1)
    except StopIteration:
        subpath = None
    try:
        (repo_url, branch) = url.split(' -b ', 1)
    except ValueError:
        branch = None
        repo_url = url
    return (repo_url, branch, subpath)


def unsplit_vcs_url(repo_url, branch=None, subpath=None):
    url = repo_url
    if branch:
        url = '%s -b %s' % (url, branch)
    if subpath:
        url = '%s [%s]' % (url, subpath)
    return url


def sanitize_url(url):
    url = url.strip()
    if url.startswith('git+http:') or url.startswith('git+https:'):
        return url[4:]
    return fixup_broken_git_url(url)


def plausible_url(url):
    return ':' in url


def fix_path_in_port(parsed, branch):
    if ':' not in parsed.netloc or parsed.netloc.endswith(']'):
        return None, None
    host, port = parsed.netloc.rsplit(':', 1)
    if host.split('@')[-1] not in ('salsa.debian.org', 'github.com'):
        return None, None
    if not port or port.isdigit():
        return None, None
    return parsed._replace(
        path='%s/%s' % (port, parsed.path.lstrip('/')),
        netloc=host), branch


def fix_salsa_scheme(parsed, branch):
    if parsed.hostname == 'salsa.debian.org':
        return parsed._replace(scheme='https'), branch
    return None, None


def fix_salsa_cgit_url(parsed, branch):
    if (parsed.hostname == 'salsa.debian.org' and
            parsed.path.startswith('/cgit/')):
        return parsed._replace(path=parsed.path[5:]), branch
    return None, None


def fix_salsa_tree_in_url(parsed, branch):
    if parsed.hostname == 'salsa.debian.org':
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


def fixup_broken_git_url(url):
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    repo_url, branch = extract_vcs_url_branch(url)

    parsed = urlparse(repo_url)
    changed = False
    for fn in [fix_path_in_port, fix_salsa_scheme, fix_salsa_cgit_url,
               fix_salsa_tree_in_url, fix_double_slash, fix_extra_colon,
               drop_git_username, fix_branch_argument]:
        newparsed, newbranch = fn(parsed, branch)
        if newparsed:
            changed = True
            parsed = newparsed
            branch = newbranch

    if changed:
        return unsplit_vcs_url(urlunparse(parsed), branch)
    return url


def browse_url_from_repo_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        path = '/'.join(parsed_url.path.split('/')[:3])
        if path.endswith('.git'):
            path = path[:-4]
        return urlunparse(
            ('https', 'github.com', path,
             None, None, None))
    if parsed_url.netloc in ('code.launchpad.net', 'launchpad.net'):
        return urlunparse(
            ('https', 'code.launchpad.net', parsed_url.path,
             parsed_url.query, parsed_url.params, parsed_url.fragment))
    if parsed_url.netloc in KNOWN_GITLAB_SITES:
        path = parsed_url.path
        if path.endswith('.git'):
            path = path[:-4]
        return urlunparse(('https', parsed_url.netloc, path, None, None, None))
    if parsed_url.netloc == 'svn.apache.org':
        path_elements = parsed_url.path.strip('/').split('/')
        if path_elements[:2] != ['repos', 'asf']:
            return None
        path_elements.pop(0)
        path_elements[0] = 'viewvc'
        return urlunparse(
            ('https', parsed_url.netloc, '/'.join(path_elements), None, None,
             None))

    return None


def determine_browser_url(vcs_type, vcs_url):
    repo_url, branch, subpath = split_vcs_url(vcs_url)
    parsed = urlparse(repo_url)
    if parsed.netloc == 'salsa.debian.org':
        from .salsa import determine_browser_url as determine_salsa_browser_url
        return determine_salsa_browser_url(vcs_url)
    if parsed.netloc == 'github.com':
        path = parsed.path.rstrip('.git')
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
            ('https', 'code.launchpad.net', path,
             parsed.query, parsed.params, parsed.fragment))
    return None


def canonicalize_vcs_browser_url(url):
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


def canonical_vcs_git_url(url):
    (repo_url, branch, subpath) = split_vcs_url(url)
    parsed_url = urlparse(repo_url)
    if parsed_url.netloc in ('salsa.debian.org', 'github.com'):
        if not parsed_url.path.rstrip('/').endswith('.git'):
            parsed_url = parsed_url._replace(
                path=parsed_url.path.rstrip('/') + '.git')
        return unsplit_vcs_url(urlunparse(parsed_url), branch, subpath)
    return url


canonicalize_vcs_fns = {
    'Browser': canonicalize_vcs_browser_url,
    'Git': canonical_vcs_git_url,
}


def canonicalize_vcs_url(vcs_type, url):
    try:
        return canonicalize_vcs_fns[vcs_type](url)
    except KeyError:
        return url
