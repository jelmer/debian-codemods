#!/usr/bin/python3

# Copyright (C) 2018 Jelmer Vernooij
#
# Parts imported from python3-iniparse:
# Copyright (c) 2001, 2002, 2003 Python Software Foundation
# Copyright (c) 2004-2008 Paramjit Oberoi <param.cs.wisc.edu>
# Copyright (c) 2007 Tim Lauridsen <tla@rasmil.dk>
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

__all__ = [
    'Undefined',
    'SystemdServiceEditor',
    'update_service_file',
    ]

from io import StringIO

from iniparse.config import (
    ConfigNamespace,
    Undefined,
    )
from iniparse.ini import (
    LineContainer,
    SectionLine,
    OptionLine,
    ContinuationLine,
    MissingSectionHeaderError,
    ParsingError,
    EmptyLine,
    CommentLine,
    make_comment,
    readline_iterator,
    )

from debmutate.reformatting import Editor

import os

LIST_KEYS = [
    'Before', 'After', 'Documentation', 'Wants', 'Alias', 'WantedBy',
    'Requires', 'RequiredBy', 'Also', 'Conflicts']


class Section(ConfigNamespace):
    _lines = None
    _options = None

    def __init__(self, lineobj):
        self._lines = [lineobj]
        self._options = {}

    def rename(self, oldkey, newkey):
        option = self._options[oldkey]
        self._options[newkey] = option
        del self._options[oldkey]
        if isinstance(option, list):
            for opt in option:
                opt.name = newkey
        else:
            option.name = newkey

    def _getitem(self, key):
        if key == '__name__':
            return self._lines[-1].name
        try:
            option = self._options[key]
        except KeyError:
            if key in LIST_KEYS:
                return OptionList(self, key, [])
            raise
        if isinstance(option, list):
            return OptionList(self, key, option)
        else:
            return option.value

    def __setitem__(self, key, value):
        if key in LIST_KEYS:
            remaining = list(value)
            for option in self._options.get(key, []):
                values = option.value.split()
                for v in list(values):
                    try:
                        remaining.remove(v)
                    except ValueError:
                        values.remove(v)
                option.value = ' '.join(values)
                if not option.value:
                    for line in self._lines:
                        line.contents.remove(option)
            if remaining:
                obj = LineContainer(
                    OptionLine(key, ' '.join(remaining), separator='='))
                self._lines[-1].add(obj)
                self._options[key].append(obj)
        else:
            if key not in self._options:
                # create a dummy object - value may have multiple lines
                obj = LineContainer(OptionLine(key, '', separator='='))
                self._lines[-1].add(obj)
                self._options[key] = obj
            # the set_value() function in LineContainer
            # automatically handles multi-line values
            self._options[key].value = value

    def __delitem__(self, key):
        for line in self._lines:
            remaining = []
            for o in line.contents:
                if isinstance(o, LineContainer):
                    if key != o.name:
                        remaining.append(o)
                else:
                    remaining.append(o)
            line.contents = remaining
        del self._options[key]

    def __iter__(self):
        d = set()
        for line in self._lines:
            for x in line.contents:
                if isinstance(x, LineContainer):
                    ans = x.name
                    if ans not in d:
                        yield ans
                        d.add(ans)

    def _new_namespace(self, name):
        raise Exception('No sub-sections allowed', name)


class OptionList(object):

    def __init__(self, section, key, options):
        self._section = section
        self._key = key
        self._options = options

    def __getitem__(self, i):
        return self._items()[i].value

    def __delitem__(self, k):
        i = 0
        for o in self._options:
            vals = o.value.split()
            if k >= i and k < i + len(vals):
                del vals[k - i]
                o.value = ' '.join(vals)
                for line in self._section._lines:
                    line.contents.remove(o)
                break
            i += len(vals)
        else:
            raise IndexError(k)

    def __setitem__(self, k, val):
        i = 0
        for o in self._options:
            vals = o.value.split()
            if k >= i and k < i + len(vals):
                vals[k - i] = val
                o.value = ' '.join(vals)
                break
            i += len(vals)
        else:
            raise IndexError(k)

    def _items(self):
        ret = []
        for o in self._options:
            ret.extend(o.value.split())
        return ret

    def __iter__(self):
        return iter(self._items())

    def __len__(self):
        return len(self._items())

    def __contains__(self, v):
        return v in self._items()

    def remove(self, value):
        for i, v in enumerate(self._items()):
            if v == value:
                del self[i]
                return
        raise ValueError(value)

    def append(self, v):
        option = OptionLine(self._key, v, separator='=')
        obj = LineContainer(option)
        self._section._lines[-1].add(obj)
        self._section._options[v].append(obj)
        self._options.append(option)


class UnitFile(ConfigNamespace):
    _data = None
    _sections = None

    def __init__(self, fp=None):
        self._data = LineContainer()
        self._sections = {}
        if fp is not None:
            self._readfp(fp)

    def _getitem(self, key):
        return self._sections[key]

    def __setitem__(self, key, value):
        raise Exception('Values must be inside sections', key, value)

    def __delitem__(self, key):
        for line in self._sections[key]._lines:
            self._data.contents.remove(line)
        del self._sections[key]

    def __iter__(self):
        d = set()
        for x in self._data.contents:
            if isinstance(x, LineContainer):
                if x.name not in d:
                    yield x.name
                    d.add(x.name)

    def _new_namespace(self, name):
        if self._data.contents:
            self._data.add(EmptyLine())
        obj = LineContainer(SectionLine(name))
        self._data.add(obj)
        if name in self._sections:
            ns = self._sections[name]
            ns._lines.append(obj)
        else:
            ns = Section(obj)
            self._sections[name] = ns
        return ns

    def __str__(self):
        return self._data.__str__()

    _line_types = [EmptyLine, CommentLine,
                   SectionLine, OptionLine,
                   ContinuationLine]

    def _parse(self, line):
        for linetype in self._line_types:
            lineobj = linetype.parse(line)
            if lineobj:
                return lineobj
        else:
            # can't parse line
            return None

    def _readfp(self, fp):
        cur_section = None
        cur_option = None
        cur_section_name = None
        cur_option_name = None
        pending_lines = []
        try:
            fname = fp.name
        except AttributeError:
            fname = '<???>'
        linecount = 0
        exc = None
        line = None
        optobj = None

        for line in readline_iterator(fp):
            lineobj = self._parse(line)
            linecount += 1

            if not cur_section and not isinstance(
                    lineobj, (CommentLine, EmptyLine, SectionLine)):
                raise MissingSectionHeaderError(fname, linecount, line)

            if lineobj is None:
                if exc is None:
                    exc = ParsingError(fname)
                exc.append(linecount, line)
                lineobj = make_comment(line)

            if isinstance(lineobj, ContinuationLine):
                if cur_option:
                    if pending_lines:
                        cur_option.extend(pending_lines)
                        pending_lines = []
                    cur_option.add(lineobj)
                else:
                    # illegal continuation line - convert to comment
                    if exc is None:
                        exc = ParsingError(fname)
                    exc.append(linecount, line)
                    lineobj = make_comment(line)

            if isinstance(lineobj, OptionLine):
                if pending_lines:
                    cur_section.extend(pending_lines)
                    pending_lines = []
                cur_option = LineContainer(lineobj)
                cur_section.add(cur_option)
                cur_option_name = cur_option.name
                optobj = self._sections[cur_section_name]
                if cur_option_name in LIST_KEYS:
                    if not cur_option.value.split():
                        # Reset list.
                        optobj._options[cur_option_name] = []
                    else:
                        optobj._options.setdefault(cur_option_name, []).append(
                            cur_option)
                else:
                    optobj._options[cur_option_name] = cur_option

            if isinstance(lineobj, SectionLine):
                self._data.extend(pending_lines)
                pending_lines = []
                cur_section = LineContainer(lineobj)
                self._data.add(cur_section)
                cur_option = None
                cur_option_name = None
                cur_section_name = cur_section.name
                if cur_section_name not in self._sections:
                    self._sections[cur_section_name] = Section(cur_section)
                else:
                    self._sections[cur_section_name]._lines.append(
                        cur_section)

            if isinstance(lineobj, (CommentLine, EmptyLine)):
                pending_lines.append(lineobj)

        self._data.extend(pending_lines)
        if line and line[-1] == '\n':
            self._data.add(EmptyLine())

        if exc:
            raise exc


def systemd_service_files(path='debian', exclude_links=True):
    """List paths to systemd service files."""
    for name in os.listdir(path):
        if not name.endswith('.service'):
            continue
        subpath = os.path.join(path, name)
        if exclude_links and os.path.islink(subpath):
            continue
        yield subpath


class SystemdServiceEditor(Editor):

    def _parse(self, content):
        return UnitFile(StringIO(self._orig_content))

    def _format(self, parsed):
        return str(parsed)

    @property
    def file(self):
        return self._parsed


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
    with SystemdServiceEditor(path) as updater:
        for section in updater.file:
            for name in updater.file[section]:
                oldval = updater.file[section][name]
                newval = cb(section, name, oldval)
                if newval != oldval:
                    updater.file[section][name] = newval


def update_service(cb):
    """Update all service files.

    Args:
      cb: Callback called with a config.ConfigObj object
    """
    for path in systemd_service_files():
        update_service_file(path, cb)
