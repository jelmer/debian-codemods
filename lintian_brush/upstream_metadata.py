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
import shlex
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


def get_python_pkg_info(path, trust_package=False):
    pkg_info = read_python_pkg_info(path)
    if pkg_info.name:
        return pkg_info
    if not trust_package:
        return pkg_info
    filename = os.path.join(path, 'setup.py')
    args = [os.path.abspath(filename), 'dist_info']
    with open(filename, 'r') as f:
        has_shebang = f.readline().startswith('#!')
    is_executable = (os.stat(filename).st_mode & 0o100 != 0)
    if not has_shebang or not is_executable:
        # TODO(jelmer): Why python3 and not e.g. python?
        args.insert(0, 'python3')

    with tempfile.TemporaryDirectory() as td:
        try:
            subprocess.call(
                args, cwd=td, stderr=subprocess.PIPE,
                stdout=subprocess.PIPE)
        except FileNotFoundError:
            pass
        return read_python_pkg_info(td)


def parse_watch_file(f):
    lines = []
    continued = ''
    for line in f:
        if line.startswith('#'):
            continue
        if not line.strip():
            continue
        if line.rstrip('\n').endswith('\\'):
            continued += line.rstrip('\n\\') + ' '
        else:
            lines.append(continued + line)
            continued = ''
    if continued:
        # Hmm, broken line?
        lines.append(continued)
    if not lines.pop(0).startswith('version='):
        pass  # Hmm, is this actually a real watch file?
    for line in lines:
        line = line.strip()
        parts = shlex.split(line)
        if parts[0].startswith('opts='):
            opts = parts[0][len('opts='):]
            yield [opts] + parts[1:]
        else:
            opts = None
            yield [opts] + parts[0:]


def guess_upstream_metadata_items(path, trust_package=False):
    """Guess upstream metadata items, in no particular order.

    Args:
      path: Path to the package
      trust: Whether to trust the package contents and i.e. run
      executables in it
    """
    if os.path.exists(os.path.join(path, 'debian/watch')):
        with open(os.path.join(path, 'debian/watch'), 'r') as f:
            for entry in parse_watch_file(f):
                url = entry[1]
                if url.startswith('https://') or url.startswith('http://'):
                    repo = guess_repo_from_url(url)
                    if repo:
                        yield "Repository", repo, "possible"
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
                yield 'Repository', repo, "possible"
        if 'XS-Go-Import-Path' in control:
            yield (
                'Repository', 'https://' + control['XS-Go-Import-Path'],
                'possible')

    if os.path.exists(os.path.join(path, 'setup.py')):
        try:
            pkg_info = get_python_pkg_info(path, trust_package=trust_package)
        except FileNotFoundError:
            pass
        else:
            if pkg_info.name:
                yield 'Name', pkg_info.name, 'certain'
            if pkg_info.home_page:
                repo = guess_repo_from_url(pkg_info.home_page)
                if repo:
                    yield 'Repository', repo, 'possible'
            for value in pkg_info.project_urls:
                url_type, url = value.split(', ')
                if url_type in ('GitHub', 'Repository'):
                    yield 'Repository', url, 'certain'

    if os.path.exists(os.path.join(path, 'package.json')):
        import json
        with open(os.path.join(path, 'package.json'), 'r') as f:
            package = json.load(f)
        if 'name' in package:
            yield 'Name', package['name'], 'certain'
        if 'repository' in package:
            if isinstance(package['repository'], dict):
                yield 'Repository', package['repository']['url'], 'certain'
            elif isinstance(package['repository'], str):
                yield 'Repository', package['repository'], 'certain'

    if os.path.exists(os.path.join(path, 'debian/copyright')):
        from debian.copyright import Copyright
        with open(os.path.join(path, 'debian/copyright'), 'r') as f:
            copyright = Copyright(f)
            header = copyright.header
        if header.upstream_name:
            yield "Name", header.upstream_name, 'certain'
        if header.upstream_contact:
            yield "Contact", ','.join(header.upstream_contact), 'certain'
        if "X-Upstream-Bugs" in header:
            yield "Bug-Database", header["X-Upstream-Bugs"], 'certain'
        if "X-Source-Downloaded-From" in header:
            yield "Repository", guess_repo_from_url(
                header["X-Source-Downloaded-From"]), 'certain'

    # TODO(jelmer): validate Repository by querying it somehow?


def guess_upstream_metadata(path, trust_package=False):
    """Guess the upstream metadata dictionary.

    Args:
      path: Path to the package
      trust_package: Whether to trust the package contents and i.e. run
          executables in it
    """
    current_certainty = {}
    code = {}
    for key, value, certainty in guess_upstream_metadata_items(
            path, trust_package=trust_package):
        if current_certainty.get(key) != 'certain':
            code[key] = value
            current_certainty[key] = certainty
    return code
