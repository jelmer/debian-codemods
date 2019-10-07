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

"""Utility functions for dealing with systemd files."""

import os
import re


def systemd_service_files(path='debian'):
    """List paths to systemd service files."""
    for name in os.listdir(path):
        if name.endswith('.service'):
            yield os.path.join(path, name)


def update_service_file(path, cb):
    """Update a service file.

    This is a fairly stupid ini-style parser; it attempts
    to preserve formatting as best as it can. Other
    ini-style parsers like configobj and configparser
    don't preserve whitespace or comments.

    Args:
      path: Path to the service file
      cb: Callback called with a config.ConfigObj object
    """
    lines = []
    changed = False
    section = None
    with open(path, 'rb') as f:
        for l in f.readlines():
            g = re.match(b'^\\[(.*)\\]$', l)
            if g:
                section = g.group(1)
                lines.append(l)
                continue
            g = re.match(b'^([A-Za-z0-9]+)=(.*)$', l)
            if g:
                name = g.group(1)
                oldval = g.group(2)
                newval = cb(section, name, oldval)
                if oldval != newval:
                    changed = True
                lines.append(name+b'='+newval+b'\n')
                continue
            lines.append(l)
    if not changed:
        return
    with open(path, 'wb') as f:
        f.writelines(lines)


def update_service(cb):
    """Update all service files.

    Args:
      cb: Callback called with a config.ConfigObj object
    """
    for path in systemd_service_files():
        if os.path.islink(path):
            continue
        update_service_file(path, cb)
