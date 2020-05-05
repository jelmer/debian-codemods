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

"""Utility functions for dealing with changelog files."""

__all__ = [
    'ChangelogParseError',
    'ChangelogCreateError',
    'ChangelogUpdater',
    ]

from io import StringIO
from debian.changelog import (
    Changelog,
    ChangelogCreateError,
    ChangelogParseError,
    )
import re
import textwrap
from typing import List, Iterator
from .reformatting import Updater


WIDTH = 80


class ChangelogUpdater(Updater):
    """Update a changelog file.

    This will only write out the changelog file if it has been changed.
    """

    def __init__(self, path='debian/changelog'):
        super(ChangelogUpdater, self).__init__(path)

    def _parse(self, content):
        return Changelog(content)

    def _format(self, parsed):
        f = StringIO()
        parsed.write_to_open_file(f)
        return f.getvalue()

    @property
    def changelog(self):
        return self._parsed


def changes_by_author(changes):
    linenos = []
    change = []
    author = None
    for i, line in enumerate(changes):
        if not line:
            continue
        m = re.fullmatch(r'  \[ (.*) \]', line)
        if m:
            author = m.group(1)
            continue
        if not line.startswith('  * '):
            change.append(line)
            linenos.append(i)
        else:
            if change:
                yield (author, linenos, change)
            change = [line]
            linenos = [i]
    if change:
        yield (author, linenos, change)


class TextWrapper(textwrap.TextWrapper):

    whitespace = r'[%s]' % re.escape('\t\n\x0b\x0c\r ')
    wordsep_simple_re = re.compile(r'(%s+)' % whitespace)

    def __init__(self, initial_indent='  * '):
        super(TextWrapper, self).__init__(
            width=WIDTH, initial_indent=initial_indent,
            subsequent_indent=' ' * len(initial_indent),
            break_long_words=False, break_on_hyphens=False)

    def _split(self, text):
        chunks = [c for c in self.wordsep_simple_re.split(text) if c]
        ret = []
        i = 0
        while i < len(chunks):
            if (any(chunks[i].endswith(x) for x in ['Closes:', 'LP:']) and
                    i+2 < len(chunks) and chunks[i+2].startswith('#')):
                ret.append('%s %s' % (chunks[i], chunks[i+2]))
                i += 3
            else:
                ret.append(chunks[i])
                i += 1
        return ret


_initial_re = re.compile(r'^[  ]+[\+\-\*] ')


def rewrap_change(change: List[str]) -> List[str]:
    if not change:
        return change
    m = _initial_re.match(change[0])
    if any([len(line) > WIDTH for line in change]) and m:
        wrapper = TextWrapper(m.group(0))
        return wrapper.wrap(
                '\n'.join(l[len(m.group(0)):] for l in change))
    else:
        return change


def rewrap_changes(changes: Iterator[str]) -> Iterator[str]:
    change: List[str] = []
    indent = None
    for line in changes:
        m = _initial_re.match(line)
        if m:
            yield from rewrap_change(change)
            change = [line]
            indent = len(m.group(0))
        elif change and line.startswith(' ' * indent):  # type: ignore
            change.append(line)
        else:
            yield from rewrap_change(change)
            change = []
            yield line
