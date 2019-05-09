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

"""Functions for working with upstream metadata."""

import os
import subprocess
import tempfile
from urllib.parse import urlparse


KNOWN_HOSTING_SITES = [
    'github.com', 'gitlab.com', 'launchpad.net', 'salsa.debian.org']


def guess_repo_from_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'github.com':
        return ('https://github.com' +
                '/'.join(parsed_url.path.split('/')[:3]))
    if parsed_url.netloc in KNOWN_HOSTING_SITES:
        return url
    return None


def read_python_pkg_info(path):
    """Get the metadata from a python setup.py file."""
    from pkginfo.utils import get_metadata
    return get_metadata(path)


def get_python_pkg_info(path):
    pkg_info = read_python_pkg_info(path)
    if pkg_info.name:
        return pkg_info
    filename = os.path.join(path, 'setup.py')
    args = [os.path.abspath(filename), 'dist_info']
    if os.stat(filename).st_mode & 0o100 == 0:
        # TODO(jelmer): Why python3 and not e.g. python
        args.insert(0, 'python3')

    with tempfile.TemporaryDirectory() as td:
        try:
            subprocess.call(
                args, cwd=td, stderr=subprocess.PIPE,
                stdout=subprocess.PIPE)
        except FileNotFoundError:
            pass
        return read_python_pkg_info(td)


def guess_upstream_metadata(path):
    """Guess the upstream metadata dictionary.
    """
    code = {}

    if os.path.exists('debian/watch'):
        with open('debian/watch', 'r') as f:
            for l in f:
                url = l.split(' ', 1)[0]
                if url.startswith('https://') or url.startswith('http://'):
                    repo = guess_repo_from_url(url)
                    if repo:
                        code["Repository"] = repo
                        break

    try:
        with open(os.path.join(path, 'debian/control'), 'r') as f:
            from debian.deb822 import Deb822
            control = Deb822(f)
    except FileNotFoundError:
        pass
    else:
        if 'Homepage' in control:
            repo = guess_repo_from_url(control['Homepage'])
            if repo:
                code['Repository'] = repo
        if 'XS-Go-Import-Path' in control:
            code['Repository'] = 'https://' + control['XS-Go-Import-Path']

    if os.path.exists('setup.py'):
        try:
            pkg_info = get_python_pkg_info(path)
        except FileNotFoundError:
            pass
        else:
            if pkg_info.name:
                code['Name'] = pkg_info.name
            if pkg_info.home_page:
                repo = guess_repo_from_url(pkg_info.home_page)
                if repo:
                    code['Repository'] = repo
            for value in pkg_info.project_urls:
                url_type, url = value.split(', ')
                if url_type in ('GitHub', 'Repository'):
                    code['Repository'] = url

    if os.path.exists('debian/copyright'):
        from debian.copyright import Copyright
        with open('debian/copyright', 'r') as f:
            copyright = Copyright(f)
            header = copyright.header
        if header.upstream_name:
            code["Name"] = header.upstream_name
        if header.upstream_contact:
            code["Contact"] = ','.join(header.upstream_contact)
        if "X-Upstream-Bugs" in header:
            code["Bug-Database"] = header["X-Upstream-Bugs"]
        if "X-Source-Downloaded-From" in header:
            code["Repository"] = guess_repo_from_url(
                header["X-Source-Downloaded-From"])

    # TODO(jelmer): validate Repository by querying it somehow?

    return code
