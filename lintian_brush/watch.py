#!/usr/bin/python3
# Copyright (C) 2019 Jelmer Vernooij <jelmer@debian.org>
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

"""Functions for working with watch files."""


import shlex
from warnings import warn


DEFAULT_VERSION = 4


class WatchFile(object):

    def __init__(self, entries=None, options=None, version=DEFAULT_VERSION):
        self.version = version
        if entries is None:
            entries = []
        self.entries = entries
        if options is None:
            options = {}
        self.options = options

    def __iter__(self):
        return iter(self.entries)

    def dump(self, f):
        def serialize_options(opts):
            s = ','.join(opts)
            if ' ' in s or '\t' in s:
                return 'opts="' + s + '"'
            return 'opts=' + s
        f.write('version=%d\n' % self.version)
        if self.options:
            f.write(serialize_options(self.options) + '\n')
        for entry in self.entries:
            if entry.options:
                f.write(serialize_options(entry.options) + ' ')
            f.write(entry.url)
            if entry.version:
                f.write(' ' + entry.version)
            if entry.script:
                f.write(' ' + entry.script)
            f.write('\n')


class Watch(object):

    def __init__(self, url, version=None, script=None, opts=None):
        self.url = url
        self.version = version
        self.script = script
        if opts is None:
            opts = {}
        self.options = opts

    def __eq__(self, other):
        if not isinstance(other, Watch):
            return False
        return (other.url == self.url and
                other.version == self.version and
                other.script == self.script and
                other.options == self.options)


class MissingVersion(Exception):
    """The version= line is missing."""


def parse_watch_file(f):
    """Parse a watch file.

    Args:
      f: watch file to parse
    """
    lines = []
    continued = ''
    for line in f:
        if line.startswith('#'):
            continue
        if not line.strip():
            continue
        if line.rstrip('\n').endswith('\\'):
            continued += line.rstrip('\n\\') + ' '
        else:
            lines.append(continued + line)
            continued = ''
    if continued:
        # Hmm, broken line?
        lines.append(continued)
    if not lines:
        return None
    firstline = lines.pop(0)
    try:
        key, value = firstline.split('=', 1)
    except ValueError:
        raise MissingVersion(f)
    if key != 'version':
        raise MissingVersion(f)
    version = int(value.strip())
    persistent_options = []
    entries = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        # TODO(jelmer): This is not actually how uscan lexes.
        parts = shlex.split(line)
        if parts[0].startswith('opts='):
            opts = parts[0][len('opts='):].split(',')
            remaining = parts[1:]
        else:
            opts = None
            remaining = parts
        if not remaining:
            persistent_options.extend(opts)
        else:
            if len(remaining) > 3:
                warn('Too many entries on line: %r' % remaining)
            entries.append(Watch(*remaining[:3], opts=opts))
    return WatchFile(
        entries=entries, options=persistent_options, version=version)
