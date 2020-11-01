#!/usr/bin/python3
# Copyright (C) 2018-2020 Jelmer Vernooij
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

"""Information about specific Debian/Ubuntu releases."""

import os
from debian.changelog import Version
from typing import Optional

_key_package_versions = None


def load_key_package_versions():
    import json
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'key-package-versions.json'))
    if not os.path.isfile(path):
        import pkg_resources
        path = pkg_resources.resource_filename(
            __name__, 'lintian-brush/key-package-versions.json')
        if not os.path.isfile(path):
            # Urgh.
            path = '/usr/share/lintian-brush/key-package-versions.json'
    with open(path, 'rb') as f:
        return json.load(f)


def key_package_version(package: str, release: str) -> Optional[Version]:
    global _key_package_versions
    if _key_package_versions is None:
        _key_package_versions = load_key_package_versions()
    # TODO(jelmer): Fall back to querying UDD directly if we can't find the
    # release?
    return _key_package_versions[package].get(release)
