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

__all__ = ['fixup_broken_git_url']

from .salsa import determine_browser_url as determine_salsa_browser_url

from urllib.parse import urlparse, urlunparse


def sanitize_url(url):
    if url.startswith('git+http:') or url.startswith('git+https:'):
        return url[4:]
    return url


def fix_path_in_port(parsed):
    if ':' not in parsed.netloc or parsed.netloc.endswith(']'):
        return None
    host, port = parsed.netloc.rsplit(':', 1)
    if host not in ('salsa.debian.org', 'github.com'):
        return None
    if not port or port.isdigit():
        return None
    if '/' not in parsed.path[1:]:
        return parsed._replace(
            path='%s/%s' % (port, parsed.path.lstrip('/')),
            netloc=host)
    return None


def fix_salsa_scheme(parsed):
    if parsed.hostname == 'salsa.debian.org':
        return parsed._replace(scheme='https')
    return None


def fix_salsa_cgit_url(parsed):
    if (parsed.hostname == 'salsa.debian.org' and
            parsed.path.startswith('/cgit/')):
        return parsed._replace(path=parsed.path[5:])
    return None


def fixup_broken_git_url(url):
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    parsed = urlparse(url)
    changed = False
    for fn in [fix_path_in_port, fix_salsa_scheme, fix_salsa_cgit_url]:
        newparsed = fn(parsed)
        if newparsed:
            changed = True
            parsed = newparsed

    if changed:
        return urlunparse(parsed)
    return url


def probe_vcs_url(url):
    from breezy.branch import Branch
    try:
        Branch.open(url).last_revision()
    except Exception:
        # TODO(jelmer): Catch more specific exceptions?
        return False
    else:
        return True


def determine_browser_url(vcs_type, vcs_url):
    parsed = urlparse(vcs_url)
    if parsed.netloc == 'salsa.debian.org':
        return determine_salsa_browser_url(vcs_url)
    if parsed.netloc == 'github.com':
        return urlunparse(
            ('https', parsed.netloc, parsed.path.rstrip('.git'),
             parsed.query, parsed.params, parsed.fragment))
    return None
