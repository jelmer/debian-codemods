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

"""Utility functions for dealing with rules files."""

import os
import re


def update_rules(command_line_cb, path='debian/rules'):
    """Update a rules file.

    Args:
      path: Path to the debian/rules file to edit
      command_line_cb: Callback to call on every command line
    Returns:
      boolean indicating whether any changes were made
    """
    if not os.path.exists(path):
        return False
    with open(path, 'rb') as f:
        original_contents = f.read()
    newlines = []
    for line in original_contents.splitlines():
        if line.startswith(b'\t'):
            newlines.append(b'\t' + command_line_cb(line[1:]))
        else:
            newlines.append(line)

    updated_contents = b''.join([l+b'\n' for l in newlines])
    if updated_contents.strip() != original_contents.strip():
        with open(path, 'wb') as f:
            f.write(updated_contents)
        return True
    return False


def dh_invoke_drop_with(line, with_argument):
    """Drop a particular value from a with argument."""
    line = re.sub(b" --with[ =]" + with_argument + b"( .+|)$", b"\\1", line)
    line = re.sub(b" --with[ =]" + with_argument + b",", b" --with=", line)
    line = re.sub(
        b" --with[ =]([^ ])," + with_argument + b"([ ,])",
        b" --with=\\1\\2", line)
    return line
