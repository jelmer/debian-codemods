#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij
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

"""Utility functions for editing files by line."""


class LineEditor:
    """Edit a file line by line."""

    def __init__(self, path, mode="", encoding='utf-8'):
        self.path = path
        self.mode = mode
        self.encoding = encoding

    def __enter__(self):
        kwargs = {}
        if not "b" in self.mode:
            kwargs["encoding"] = self.encoding
        with open(self.path, "r" + self.mode, **kwargs) as f:
            self._oldlines = list(f)
        self._newlines = list(self._oldlines)
        return self

    def done(self) -> None:
        """Write out the file if it has changed."""
        if self._oldlines != self._newlines:
            with open(self.path, "w" + self.mode) as f:
                f.writelines(self._newlines)

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            self.done()
        return False

    def __iter__(self):
        return enumerate(self._newlines, 1)

    def __setitem__(self, i: int, newline):
        self._newlines[i - 1] = newline

    def __getitem__(self, i: int):
        return self._newlines[i - 1]

    def __delitem__(self, i: int):
        del self._newlines[i - 1]

    def insert(self, lineno: int, line):
        """Insert a new line."""
        self._newlines.insert(lineno - 1, line)

    def __len__(self) -> int:
        return len(self._newlines)
