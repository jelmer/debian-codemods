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

from io import StringIO

from iniparse.ini import INIConfig, LineContainer, OptionLine

from .reformatting import edit_formatted_file

import os


def systemd_service_files(path='debian'):
    """List paths to systemd service files."""
    for name in os.listdir(path):
        if name.endswith('.service'):
            yield os.path.join(path, name)


class SystemdServiceUpdater(object):

    def __init__(self, path):
        self.path = path

    def __enter__(self):
        with open(self.path, 'r') as f:
            self._orig_content = f.read()

        self.file = INIConfig(
            StringIO(self._orig_content), optionxformvalue=lambda x: x)
        self._rewritten_content = self.dump()
        return self

    def dump(self):
        return str(self.file)

    def __exit__(self, exc_type, exc_val, exc_tb):
        updated_content = self.dump()

        self.changed = edit_formatted_file(
            self.path, self._orig_content, self._rewritten_content,
            updated_content)
        return False


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
    with SystemdServiceUpdater(path) as updater:
        for section in updater.file:
            for name in updater.file[section]:
                oldval = updater.file[section][name]
                newval = cb(section, name, oldval)
                updater.file[section][name] = newval


def update_service(cb):
    """Update all service files.

    Args:
      cb: Callback called with a config.ConfigObj object
    """
    for path in systemd_service_files():
        if os.path.islink(path):
            continue
        update_service_file(path, cb)
