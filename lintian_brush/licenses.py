#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij
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

"""Utility functions for dealing with licenses."""

import json
import os

COMMON_LICENSES_DIR = '/usr/share/common-licenses'

FULL_LICENSE_NAME = {
    'Apache-2.0': 'Apache License, Version 2.0',
    'CC0-1.0': 'CC0 1.0 Universal license',
}


def load_spdx_data():
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'spdx.json'))
    if not os.path.isfile(path):
        import pkg_resources
        path = pkg_resources.resource_filename(
            __name__, 'lintian-brush/spdx.json')
        if not os.path.isfile(path):
            # Urgh.
            path = '/usr/share/lintian-brush/spdx.json'
    with open(path, 'rb') as f:
        return json.load(f)
