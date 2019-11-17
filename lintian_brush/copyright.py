#!/usr/bin/python3
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

"""Utility functions for dealing with copyright files."""

__all__ = [
    'NotMachineReadableError',
    'MachineReadableFormatError',
    'update_copyright',
    ]

from debian.copyright import (
    Copyright,
    MachineReadableFormatError,
    NotMachineReadableError,
    )

from .reformatting import edit_formatted_file


class CopyrightUpdater(object):

    def __init__(self, path='debian/copyright'):
        self.path = path

    def __enter__(self):
        with open(self.path, 'r') as f:
            self._orig_content = f.read()

        self.copyright = Copyright(self._orig_content, strict=False)
        self._rewritten_content = self.copyright.dump()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        updated_content = self.copyright.dump()

        self.changed = edit_formatted_file(
            self.path, self._orig_content, self._rewritten_content,
            updated_content)
        return False


def update_copyright(update_cb, path='debian/copyright'):
    """Update a machine-readable copyright file.

    Args:
      update_cb: Callback that will be called with the
        copyright file
      path: Optional path to the copyright file
    Returns:
      bool indicating whether any changes were made
    """
    with CopyrightUpdater(path=path) as updater:
        update_cb(updater.copyright)
    return updater.changed


def upstream_fields_in_copyright(path='debian/copyright'):
    ret = []
    try:
        with open(path, 'r') as f:
            c = Copyright(f)
    except (FileNotFoundError, NotMachineReadableError,
            MachineReadableFormatError):
        return []
    else:
        if c.header.upstream_contact:
            ret.append('Contact')
        if c.header.upstream_name:
            ret.append('Name')
    return ret
