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
from ruamel.yaml import YAML


class YamlUpdater(object):

    def __init__(self, path, remove_empty=True):
        self.yaml = YAML()
        self.path = path
        self._dirpath = os.path.dirname(path)
        self.remove_empty = remove_empty
        self._directives = []

    def __enter__(self):
        try:
            with open(self.path, 'r') as f:
                inp = list(f)
        except FileNotFoundError:
            self._code = {}
        else:
            if '---\n' in inp:
                for i, line in enumerate(inp):
                    if line == '---\n':
                        self._directives = inp[:i+1]
                        break
            self._code = self.yaml.load(''.join(inp))
        self._orig = copy.deepcopy(self._code)
        return self._code

    def __exit__(self, exc_type, exc_val, exc_tb):
        if not exc_type:
            if not self._code and self.remove_empty:
                if os.path.exists(self.path):
                    os.unlink(self.path)
                    if self._dirpath and not os.listdir(self._dirpath):
                        os.rmdir(self._dirpath)
            else:
                if self._code != self._orig:
                    if not os.path.exists(self._dirpath) and self._dirpath:
                        os.mkdir(self._dirpath)
                    with open(self.path, 'w') as f:
                        f.writelines(self._directives)
                        self.yaml.dump(self._code, f)
        return False
