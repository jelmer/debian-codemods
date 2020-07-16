#!/usr/bin/python
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

"""Fix issues in XDG files."""

from iniparse.ini import INIConfig, LineContainer, OptionLine
from io import StringIO

from debmutate.reformatting import edit_formatted_file


class DesktopEntryEditor(object):

    def __init__(self, path):
        self.path = path

    def __enter__(self):
        with open(self.path, 'r') as f:
            self._orig_content = f.read()

        self._entry = INIConfig(StringIO(self._orig_content))
        self.entry = self._entry['Desktop Entry']
        self._rewritten_content = self.dump()
        return self

    def get(self, name):
        try:
            return self.entry[name]
        except KeyError:
            return None

    def __getitem__(self, name):
        return self.entry[name]

    def __delitem__(self, name):
        del self.entry[name]

    def __setitem__(self, key, value):
        if key not in self.entry._options:
            lc = LineContainer(OptionLine(key, value, separator='='))
            self.entry._options[key] = lc
            self.entry._lines[-1].add(lc)
        else:
            self.entry._options[key].value = value

    def dump(self):
        return str(self._entry)

    def __exit__(self, exc_type, exc_val, exc_tb):
        updated_content = self.dump()

        self.changed = edit_formatted_file(
            self.path, self._orig_content, self._rewritten_content,
            updated_content)
        return False
