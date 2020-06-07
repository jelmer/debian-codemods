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
from typing import Iterable, List, Union, Callable, Optional, TextIO, Iterator
from warnings import warn

from .reformatting import (
    Updater,
    )


DEFAULT_VERSION: int = 4

# TODO(jelmer): Add support for substitution variables:

SUBSTITUTIONS = {
    # This is substituted with the source package name found in the first line
    # of the debian/changelog file.
    '@PACKAGE': None,
    # This is substituted by the legal upstream version regex (capturing).
    '@ANY_VERSION@': r'[-_]?(\d[\-+\.:\~\da-zA-Z]*)',
    # This is substituted by the typical archive file extension regex
    # (non-capturing).
    '@ARCHIVE_EXT@': r'(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)',
    # This is substituted by the typical signature file extension regex
    # (non-capturing).
    '@SIGNATURE_EXT@':
        r'(?i)\.(?:tar\.xz|tar\.bz2|tar\.gz|zip|tgz|tbz|txz)'
        r'\.(?:asc|pgp|gpg|sig|sign)',
    # This is substituted by the typical Debian extension regexp (capturing).
    '@DEB_EXT@': r'[\+~](debian|dfsg|ds|deb)(\.)?(\d+)?$',
}


class WatchFile(object):

    def __init__(self, entries: Optional[List['Watch']] = None,
                 options: Optional[List[str]] = None,
                 version: int = DEFAULT_VERSION) -> None:
        self.version = version
        if entries is None:
            entries = []
        self.entries = entries
        if options is None:
            options = []
        self.options = options

    def __iter__(self) -> Iterator['Watch']:
        return iter(self.entries)

    def __bool__(self) -> bool:
        return bool(self.entries)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, type(self)) and \
                self.entries == other.entries and \
                self.options == other.options and \
                self.version == other.version

    def dump(self, f: TextIO) -> None:
        def serialize_options(opts: List[str]) -> str:
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

    def __init__(self, url: str, matching_pattern: Optional[str] = None,
                 version: Optional[str] = None, script: Optional[str] = None,
                 opts: Optional[List[str]] = None) -> None:
        self.url = url
        self.matching_pattern = matching_pattern
        self.version = version
        self.script = script
        if opts is None:
            opts = []
        self.options = opts

    def __repr__(self) -> str:
        return (
            "%s(%r, matching_pattern=%r, version=%r, script=%r, opts=%r)" % (
                self.__class__.__name__, self.url, self.matching_pattern,
                self.version, self.script, self.options))

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Watch):
            return False
        return (other.url == self.url and
                other.matching_pattern == self.matching_pattern and
                other.version == self.version and
                other.script == self.script and
                other.options == self.options)

    def format_url(self, package: Union[str, Callable[[], str]]) -> str:
        if '@PACKAGE@' not in self.url:
            return self.url
        if callable(package):
            package = package()
        return self.url.replace('@PACKAGE@', package)


class MissingVersion(Exception):
    """The version= line is missing."""


def parse_watch_file(f: Iterable[str]) -> Optional[WatchFile]:
    """Parse a watch file.

    Args:
      f: watch file to parse
    """
    line: Optional[str]
    lines: List[List[str]] = []
    continued: List[str] = []
    for line in f:
        if line.startswith('#'):
            continue
        if not line.strip():
            continue
        if line.rstrip('\n').endswith('\\'):
            continued.append(line.rstrip('\n\\'))
        else:
            continued.append(line)
            lines.append(continued)
            continued = []
    if continued:
        # Hmm, broken line?
        warn('watchfile ended with \\; skipping last line')
        lines.append(continued)
    if not lines:
        return None
    firstline = ''.join(lines.pop(0))
    try:
        key, value = firstline.split('=', 1)
    except ValueError:
        raise MissingVersion()
    if key.strip() != 'version':
        raise MissingVersion()
    version = int(value.strip())
    persistent_options: List[str] = []
    entries: List[Watch] = []
    chunked: List[str]
    for chunked in lines:
        if version > 3:
            chunked = [chunk.lstrip() for chunk in chunked]
        line = ''.join(chunked).strip()
        if not line:
            continue
        opts: Optional[List[str]]
        if line.startswith('opts='):
            if line[5] == '"':
                optend = line.index('"', 6)
                if optend == -1:
                    raise ValueError('Not matching " in %r' % line)
                opts_str = line[6:optend]
                line = line[optend+1:]
            else:
                try:
                    (opts_str, line) = line[5:].split(maxsplit=1)
                except ValueError:
                    opts_str = line[5:]
                    line = None
            opts = opts_str.split(',')
        else:
            opts = None
        if not line:
            if opts:
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
            entries.append(Watch(url, *parts, opts=opts))  # type: ignore
    return WatchFile(
        entries=entries, options=persistent_options, version=version)


class WatchUpdater(Updater):

    _parsed: WatchFile

    def __init__(self, path: str = 'debian/watch') -> None:
        super(WatchUpdater, self).__init__(path)

    @property
    def watch_file(self) -> WatchFile:
        return self._parsed

    def _nonexistant(self) -> None:
        return None

    def _parse(self, content: str) -> WatchFile:
        wf = parse_watch_file(content.splitlines())
        if wf is None:
            return WatchFile([])
        return wf

    def _format(self, parsed: WatchFile) -> str:
        if parsed is None:
            return None
        nf = StringIO()
        parsed.dump(nf)
        return nf.getvalue()
