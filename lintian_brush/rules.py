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
    target = None
    for line in original_contents.splitlines():
        if line.startswith(b'\t'):
            ret = command_line_cb(line[1:], target)
            if isinstance(ret, bytes):
                newlines.append(b'\t' + ret)
            elif isinstance(ret, list):
                newlines.extend([b'\t' + l for l in ret])
            else:
                raise TypeError(ret)
        elif b':' in line:
            target = line.split(b':')[0]
            newlines.append(line)
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


def dh_invoke_drop_argument(line, argument):
    """Drop a particular argument from a dh invocation."""
    if argument not in line:
        return False
    line = re.sub(b' ' + argument + b'$', b'', line)
    line = re.sub(b' ' + argument + b' ', b' ', line)
    return line


def check_cdbs(path='debian/rules'):
    if not os.path.exists(path):
        return False
    with open(path, 'rb') as f:
        for line in f:
            if line.lstrip(b'-').startswith(b'include /usr/share/cdbs/'):
                return True
    return False
