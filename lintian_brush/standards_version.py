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


from typing import Tuple, Iterator


RELEASE_DATES_PATH = '/usr/share/lintian/data/standards-version/release-dates'


def parse_standards_version(v: str) -> Tuple[int, ...]:
    """Parse a standards version.

    Args:
      v: Version string
    Returns: Tuple with version
    """
    return tuple([int(k) for k in v.split('.')])


def iter_standards_versions() -> Iterator[Tuple[Tuple[int, ...], int]]:
    with open(RELEASE_DATES_PATH, 'r') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            (version, ts) = line.split()
            yield parse_standards_version(version), int(ts)
