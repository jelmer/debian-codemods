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

from io import StringIO
import re

from lintian_brush.reformatting import (
    check_generated_file,
    edit_formatted_file,
    )


DEFAULT_VERSION = 4


class WatchFile(object):

    def __init__(self, entries=None, options=None, version=DEFAULT_VERSION):
        self.version = version
        if entries is None:
            entries = []
        self.entries = entries
        if options is None:
            options = []
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
            if entry.matching_pattern:
                f.write(' ' + entry.matching_pattern)
            if entry.version:
                f.write(' ' + entry.version)
            if entry.script:
                f.write(' ' + entry.script)
            f.write('\n')


class Watch(object):

    def __init__(self, url, matching_pattern=None, version=None, script=None,
                 opts=None):
        self.url = url
        self.matching_pattern = matching_pattern
        self.version = version
        self.script = script
        if opts is None:
            opts = {}
        self.options = opts

    def __repr__(self):
        return (
            "%s(%r, matching_pattern=%r, version=%r, script=%r, opts=%r)" % (
                self.__class__.__name__, self.url, self.matching_pattern,
                self.version, self.script, self.options))

    def __eq__(self, other):
        if not isinstance(other, Watch):
            return False
        return (other.url == self.url and
                other.matching_pattern == self.matching_pattern and
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
        raise MissingVersion()
    if key.strip() != 'version':
        raise MissingVersion()
    version = int(value.strip())
    persistent_options = []
    entries = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        if line.startswith('opts='):
            if line[5] == '"':
                optend = line.index('"', 6)
                if optend == -1:
                    raise ValueError('Not matching " in %r' % line)
                opts = line[6:optend]
                line = line[optend+1:]
            else:
                try:
                    (opts, line) = line[5:].split(maxsplit=1)
                except ValueError:
                    opts = line[5:]
                    line = None
            opts = opts.split(',')
        else:
            opts = None
        if not line:
            persistent_options.extend(opts)
        else:
            try:
                url, line = line.split(maxsplit=1)
            except ValueError:
                url = line
                line = ''
            m = re.findall(r'/([^/]*\([^/]*\)[^/]*)$', url)
            if m:
                parts = [m[0]] + line.split(maxsplit=1)
                url = url[:-len(m[0])-1]
            else:
                parts = line.split(maxsplit=2)
            entries.append(Watch(url, *parts, opts=opts))
    return WatchFile(
        entries=entries, options=persistent_options, version=version)


def update_watch(update_entry=None, path='debian/watch'):
    check_generated_file(path)
    try:
        with open(path, 'r') as f:
            original_contents = f.read()
    except FileNotFoundError:
        return False
    wf = parse_watch_file(original_contents.splitlines())
    if wf is None:
        return False
    nf = StringIO()
    wf.dump(nf)
    rewritten_contents = nf.getvalue()
    for entry in wf.entries:
        if update_entry is not None:
            update_entry(entry)
    nf = StringIO()
    wf.dump(nf)
    updated_contents = nf.getvalue()
    return edit_formatted_file(
        path, original_contents, rewritten_contents, updated_contents)
