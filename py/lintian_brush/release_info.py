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

from typing import Optional

from debian.changelog import Version


def key_package_version(package: str, release: str) -> Optional[Version]:
    """Return the version of a package in a specific release."""
    from ._lintian_brush_rs import DEBHELPER_VERSIONS, DPKG_VERSIONS

    if package == "dpkg":
        return DPKG_VERSIONS.get(release)
    if package == "debhelper":
        return DEBHELPER_VERSIONS.get(release)
    raise ValueError(f"Unknown package {package!r} for release {release!r}")
