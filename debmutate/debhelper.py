#!/usr/bin/python3
# Copyright (C) 2019 Jelmer Vernooij
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


"""Debhelper utility functions."""

import os
from typing import Optional

from debian.deb822 import Deb822

from debian.changelog import Version

from .control import ensure_minimum_version, get_relation


def ensure_minimum_debhelper_version(source, minimum_version):
    """Ensure that the pakcage is at least using version x of debhelper.

    This is a dedicated helper, since debhelper can now also be pulled in
    with a debhelper-compat dependency.

    Args:
      source: Source dictionary
      version: The minimum version
    """
    # TODO(jelmer): Also check Build-Depends-Indep and Build-Depends-Arch?
    for field in ['Build-Depends-Arch', 'Build-Depends-Indep']:
        value = source.get(field, '')
        try:
            offset, debhelper_compat = get_relation(
                value, "debhelper-compat")
        except KeyError:
            pass
        else:
            raise Exception('debhelper-compat in %s' % field)
        try:
            offset, debhelper_compat = get_relation(
                value, "debhelper")
        except KeyError:
            pass
        else:
            raise Exception('debhelper compat in %s' % field)

    build_depends = source.get('Build-Depends', '')
    minimum_version = Version(minimum_version)
    try:
        offset, debhelper_compat = get_relation(
            build_depends, "debhelper-compat")
    except KeyError:
        pass
    else:
        if len(debhelper_compat) > 1:
            raise Exception("Complex rule for debhelper-compat, aborting")
        if debhelper_compat[0].version[0] != '=':
            raise Exception("Complex rule for debhelper-compat, aborting")
        if Version(debhelper_compat[0].version[1]) >= minimum_version:
            return False
    new_build_depends = ensure_minimum_version(
            build_depends,
            "debhelper", minimum_version)
    if new_build_depends != source.get('Build-Depends'):
        source['Build-Depends'] = new_build_depends
        return True
    return False


def read_debhelper_compat_file(path):
    """Read a debian/compat file.

    Args:
      path: Path to read from
    """
    with open(path, 'r') as f:
        line = f.readline().split('#', 1)[0]
        return int(line.strip())


def get_debhelper_compat_level(path: str = '.') -> Optional[int]:
    try:
        return read_debhelper_compat_file(os.path.join(path, 'debian/compat'))
    except FileNotFoundError:
        pass

    try:
        with open(os.path.join(path, 'debian/control'), 'r') as f:
            control = Deb822(f)
    except FileNotFoundError:
        return None

    try:
        offset, [relation] = get_relation(
            control.get("Build-Depends", ""), "debhelper-compat")
    except (IndexError, KeyError):
        return None
    else:
        return int(str(relation.version[1]))
