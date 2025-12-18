#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij <jelmer@debian.org>
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

"""Functions for working with lintian data."""

import os
from typing import Optional, TextIO

LINTIAN_DATA_PATH = "/usr/share/lintian/data"


OBSOLETE_SITES_PATH = os.path.join(
    LINTIAN_DATA_PATH, "obsolete-sites/obsolete-sites"
)
_obsolete_sites = None


def _read_list_file(f: TextIO, vendor: Optional[str] = None):
    for line in f:
        line = line.strip()
        if not line:
            continue
        if line.startswith("#"):
            continue
        if line.startswith("@"):
            cond, if_vendor, val = line.split(None, 2)
            if cond == "@if-vendor-is-not":
                if (
                    if_vendor
                    and vendor
                    and if_vendor.lower() == vendor.lower()
                ):
                    continue
                line = val
            else:
                raise ValueError(f"invalid check {cond!r}")
        yield line


def is_obsolete_site(parsed_url) -> Optional[str]:
    global _obsolete_sites
    if _obsolete_sites is None:
        with open(OBSOLETE_SITES_PATH) as f:
            _obsolete_sites = list(_read_list_file(f, vendor=None))
    for site in _obsolete_sites:
        if parsed_url.hostname.endswith(site):
            return site
    else:
        return None
