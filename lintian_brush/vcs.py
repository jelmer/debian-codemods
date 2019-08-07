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

from urllib.parse import urlparse, urlunparse


def fixup_broken_git_url(url):
    """Attempt to fix up broken Git URLs.

    A common misspelling is to add an extra ":" after the hostname
    """
    parsed = urlparse(url)
    path = parsed.path
    scheme = parsed.scheme
    if '@' in parsed.netloc:
        credentials, host = parsed.netloc.rsplit('@', 1)
    else:
        credentials = None
        host = parsed.netloc

    if ':' in host and not (host[0] == '[' and host[-1] == ']'):
        # there *is* port
        host, port = host.rsplit(':', 1)
        if not port or port.isdigit():
            return url
    if host in ('salsa.debian.org', 'github.com'):
        if '/' not in path[1:]:
            path = '%s/%s' % (port, path.lstrip('/'))
        netloc = host
        if ":" in netloc:
            netloc = "[%s]" % netloc
        if credentials is not None:
            netloc = '%s@%s' % (credentials, netloc)
        if host == 'salsa.debian.org':
            scheme = 'https'
        if host == 'salsa.debian.org' and path.startswith('/cgit/'):
            path = path[5:]
        new_url = urlunparse(
            (scheme, host, path, parsed.params, parsed.query, parsed.fragment))
        return new_url
    return url
