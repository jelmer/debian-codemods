#!/usr/bin/python
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

"""Standards-Version handling."""

import json
import os
from datetime import datetime
from typing import Iterator, Tuple

from debmutate.control import parse_standards_version
from iso8601 import parse_date

from .lintian import LINTIAN_DATA_PATH

RELEASE_DATES_PATH = os.path.join(
    LINTIAN_DATA_PATH, "debian-policy/releases.json")
OLD_RELEASE_DATES_PATH = os.path.join(
    LINTIAN_DATA_PATH, "standards-version/release-dates")


def iter_standards_versions() -> Iterator[Tuple[Tuple[int, ...], datetime]]:
    try:
        with open(RELEASE_DATES_PATH) as f:
            data = json.load(f)
            for release in data['releases']:
                version = release['version']
                yield (
                    parse_standards_version(version),
                    parse_date(release['timestamp']))
    except FileNotFoundError:
        with open(OLD_RELEASE_DATES_PATH) as f:
            for line in f:
                if line.startswith("#") or not line.strip():
                    continue
                (version, ts) = line.split()
                yield (parse_standards_version(version),
                       datetime.fromtimestamp(int(ts)))


def latest_standards_version() -> str:
    return ".".join(map(str, next(iter_standards_versions())[0]))
