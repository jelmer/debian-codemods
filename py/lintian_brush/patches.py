#!/usr/bin/python3
# Copyright (C) 2024 Jelmer Vernooij
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

"""Utility functions for dealing with patches."""

__all__ = [
    'find_patches_directory',
    'tree_has_non_patches_changes',
    'read_quilt_series',
]

from ._lintian_brush_rs import (
    tree_has_non_patches_changes,
)
import os


DEFAULT_DEBIAN_PATCHES_DIR = "debian/patches"

def rules_find_patches_directory(makefile):
    """Find the patches directory set in debian/rules.

    Args:
        makefile: Makefile to scan
    Returns:
        path to patches directory, or None if none was found in debian/rules
    """
    try:
        val = makefile.get_variable(b"QUILT_PATCH_DIR")
    except KeyError:
        return None
    else:
        return val.decode()


def find_patches_directory(path):
    """Find the name of the patches directory, if any.

    Args:
      path: Root to package
    Returns:
      relative path to patches directory, or None if none exists
    """
    from debmutate._rules import Makefile

    directory = None
    try:
        mf = Makefile.from_path(os.path.join(path, "debian/rules"))
    except FileNotFoundError:
        pass
    else:
        rules_directory = rules_find_patches_directory(mf)
        if rules_directory is not None:
            directory = rules_directory
    if directory is None and os.path.exists(
        os.path.join(path, DEFAULT_DEBIAN_PATCHES_DIR)
    ):
        directory = DEFAULT_DEBIAN_PATCHES_DIR
    return directory


# TODO(jelmer): Use debmutate version
def read_quilt_series(f):
    for line in f:
        if line.startswith(b'#'):
            quoted = True
            line = line.split(b'#')[1].strip()
        else:
            quoted = False
        args = line.decode().split()
        if not args:
            continue
        patch = args[0]
        if not patch:
            continue
        options = args[1:]
        yield patch, quoted, options
