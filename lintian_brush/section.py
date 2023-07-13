#!/usr/bin/python
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

"""Section handling."""

import os
import re

from .lintian import LINTIAN_DATA_PATH

NAME_SECTION_MAPPINGS_PATH = os.path.join(
    LINTIAN_DATA_PATH, "fields/name_section_mappings")


def get_name_section_mappings():
    regexes = []

    with open(NAME_SECTION_MAPPINGS_PATH) as f:
        for line in f:
            if line.startswith('#'):
                continue
            if not line.strip():
                continue
            (regex, section) = line.split("=>")
            regexes.append((re.compile(regex.strip()), section.strip()))
    return regexes


def find_expected_section(regexes, name):
    for regex, section in regexes:
        if regex.match(name):
            return section
    return None
