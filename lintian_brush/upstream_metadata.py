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
import urllib.parse


KNOWN_HOSTING_SITES = [
    'github.com', 'gitlab.com', 'launchpad.net', 'salsa.debian.org']


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

    try:
        with open(os.path.join(path, 'debian/control'), 'r') as f:
            from debian.deb822 import Deb822
            control = Deb822(f)
    except FileNotFoundError:
        pass
    else:
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
                parsed_url = urllib.parse.urlparse(pkg_info.home_page)
                if parsed_url.netloc in KNOWN_HOSTING_SITES:
                    code['Repository'] = pkg_info.home_page
            for value in pkg_info.project_urls:
                url_type, url = value.split(', ')
                if url_type in ('GitHub', 'Repository'):
                    code['Repository'] = url

    # TODO(jelmer): validate Repository by querying it somehow?

    return code
