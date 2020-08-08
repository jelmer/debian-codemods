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
from io import StringIO
import os
from ruamel.yaml import YAML
from typing import List


class YamlUpdater(object):

    def __init__(
            self, path: str, remove_empty: bool = True,
            allow_duplicate_keys: bool = False):
        self.yaml = YAML()
        self.yaml.allow_duplicate_keys = allow_duplicate_keys
        self.path = path
        self._dirpath = os.path.dirname(path)
        self.remove_empty = remove_empty
        self._directives: List[str] = []
        self._force_rewrite = False

    def force_rewrite(self):
        self._force_rewrite = True

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
        return self

    def _get_code(self):
        return self._code

    def _set_code(self, v):
        self._code = v

    code = property(_get_code, _set_code)

    def _only_simple_changes(self):
        if not isinstance(self._orig, dict):
            return False

        def is_one_line(k, v):
            f = StringIO()
            self.yaml.dump({k: v}, f)
            return len(f.getvalue().splitlines()) == 1
        # Check if there are only "simple" changes, i.e.
        # changes that update string fields
        for k, v in self._code.items():
            if v == self._orig.get(k) or not is_one_line(k, v):
                continue
        for k, v in self._orig.items():
            if k not in self._code and not is_one_line(k, v):
                return False
        return True

    def _update_lines(self, lines, f):
        for line in self._directives:
            f.write(line)
        os = list(self._orig.keys())
        cs = list(self._code.keys())
        o = 0
        for line in lines[len(self._directives):]:
            try:
                key = os[o]
            except IndexError:
                key = None
            if key and line.startswith(key + ':'):
                while cs and cs[0] not in os:
                    self.yaml.dump({cs[0]: self._code[cs[0]]}, f)
                    cs.pop(0)
                if key not in self._code:
                    pass  # Line was removed
                elif self._code[key] == self._orig[key]:
                    # Line stayed the same
                    f.write(line)
                    cs.remove(key)
                else:
                    self.yaml.dump({key: self._code[key]}, f)
                    cs.remove(key)
                o += 1
            else:
                f.write(line)
        if cs and not line.endswith('\n'):
            f.write('\n')
        while cs:
            key = cs.pop(0)
            self.yaml.dump({key: self._code[key]}, f)

    def __exit__(self, exc_type, exc_val, exc_tb):
        if not exc_type:
            if not self._code and self.remove_empty:
                if os.path.exists(self.path):
                    os.unlink(self.path)
                    if self._dirpath and not os.listdir(self._dirpath):
                        os.rmdir(self._dirpath)
            else:
                if self._force_rewrite or self._code != self._orig:
                    if not os.path.exists(self._dirpath) and self._dirpath:
                        os.mkdir(self._dirpath)
                    if not self._force_rewrite and self._only_simple_changes():
                        try:
                            with open(self.path, 'r') as f:
                                lines = list(f.readlines())
                        except FileNotFoundError:
                            lines = ['---\n']
                        with open(self.path, 'w') as f:
                            self._update_lines(lines, f)
                    else:
                        with open(self.path, 'w') as f:
                            f.writelines(self._directives)
                            self.yaml.dump(self._code, f)
        return False


def update_ordered_dict(code, changed, key=None):
    if key is None:
        def key(x):
            return x

    to_insert = []
    for k, v in changed:
        if k in code:
            code[k] = v
        else:
            to_insert.append((k, v))

    to_insert.sort(key=key)
    i = 0
    for k, v in list(code.items()):
        while to_insert and key((k, v)) > key(to_insert[0]):
            code.insert(i, *to_insert.pop(0))
            i += 1
        i += 1

    while to_insert:
        k, v = to_insert.pop(0)
        code[k] = v
