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

"""Utility functions for dealing with YAML files."""

import copy
import os
import ruamel.yaml


def write_yaml_file(path, code):
    with open(path, 'w') as f:
        ruamel.yaml.round_trip_dump(code, f)


class YamlUpdater(object):

    def __init__(self, path, remove_empty=True):
        self.path = path
        self.remove_empty = remove_empty

    def __enter__(self):
        try:
            with open(self.path, 'r') as f:
                inp = f.read()
        except FileNotFoundError:
            self._code = {}
        else:
            self._code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)
        self._orig = copy.deepcopy(self._code)
        return self._code

    def __exit__(self, exc_type, exc_val, exc_tb):
        if not self._code and self.remove_empty:
            if os.path.exists(self.path):
                os.unlink(self.path)
        else:
            if self._code != self._orig:
                write_yaml_file(self.path, self._code)
        return False
