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

from datetime import datetime
from email.utils import format_datetime
from io import StringIO
from debian.changelog import (
    Changelog,
    ChangelogCreateError,
    ChangelogParseError,
    get_maintainer,
    Version,
    )
from email.utils import parseaddr
import re
import textwrap
from typing import List, Iterator, Tuple, Optional
from debmutate.reformatting import Updater


WIDTH = 80
INITIAL_INDENT = '  * '


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


def changes_sections(
        changes: List[str]
        ) -> Iterator[
            Tuple[Optional[str], List[int], List[List[Tuple[int, str]]]]
        ]:
    section: Tuple[
        Optional[str], List[int],
        List[List[Tuple[int, str]]]] = (None, [], [])
    change: List[Tuple[int, str]] = []
    for i, line in enumerate(changes):
        if not line and i == 0:
            # Skip the first line
            continue
        if not line:
            section[1].append(i)
            continue
        m = re.fullmatch(r'  \[ (.*) \]', line)
        if m:
            if change:
                section[2].append(change)
                change = []
            if section[1]:
                yield section
            section = (m.group(1), [i], [])
            continue
        if not line.startswith(INITIAL_INDENT):
            change.append((i, line))
            section[1].append(i)
        else:
            if change:
                section[2].append(change)
            change = [(i, line)]
            section[1].append(i)
    if change:
        section[2].append(change)
    if section[1]:
        yield section


def changes_by_author(
        changes: List[str]
        ) -> Iterator[Tuple[Optional[str], List[int], List[str]]]:
    """Changes by author.

    Args:
      changes: Find changes by author from a list of lines
    Returns:
      Iterator over items by author (maintainer, offsets, changes)
    """
    for (author, linenos, contents) in changes_sections(changes):
        for change_entries in contents:
            change_linenos, change_lines = zip(*change_entries)
            yield (author, change_linenos, change_lines)  # type: ignore


class TextWrapper(textwrap.TextWrapper):

    whitespace = r'[%s]' % re.escape('\t\n\x0b\x0c\r ')
    wordsep_simple_re = re.compile(r'(%s+)' % whitespace)

    def __init__(self, initial_indent=INITIAL_INDENT):
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


def _can_join(line1, line2):
    if line1.endswith(':'):
        return False
    if line2 and line2[:1].isupper():
        if line1.endswith(']') or line1.endswith('}'):
            return False
        if not line1.endswith('.'):
            return False
    return True


def any_long_lines(lines, width=WIDTH):
    return any([len(line) > width for line in lines])


def rewrap_change(change: List[str]) -> List[str]:
    if not change:
        return change
    m = _initial_re.match(change[0])
    if not any_long_lines(change) or not m:
        return change
    wrapper = TextWrapper(m.group(0))
    prefix_len = len(m.group(0))
    lines = [line[prefix_len:] for line in change]
    todo = [lines[0]]
    ret = []
    for i in range(len(lines) - 1):
        if _can_join(lines[i], lines[i+1]) and any_long_lines(
                todo, WIDTH-prefix_len):
            todo.append(lines[i+1])
        else:
            ret.extend(wrapper.wrap('\n'.join(todo)))
            wrapper = TextWrapper(change[i+1][:len(m.group(0))])
            todo = [lines[i+1]]
    ret.extend(wrapper.wrap('\n'.join(todo)))
    return ret


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


def _inc_version(version: Version) -> Version:
    """Increment a Debian version string."""
    ret = Version(str(version))
    # TODO(jelmer): Add ubuntuX suffix on Ubuntu
    if ret.debian_revision:
        # Non-native package
        m = re.match('^(.*?)([0-9]+)$', ret.debian_revision)
        if m:
            ret.debian_revision = m.group(1) + str(int(m.group(2))+1)
        else:
            ret.debian_revision += "1"
        return ret
    else:
        # Native package
        m = re.match('^(.*?)([0-9]+)$', ret.upstream_version)
        if m:
            ret.upstream_version = m.group(1) + str(int(m.group(2))+1)
        else:
            ret.upstream_version += "1"
        return ret


def changelog_add_entry(
        cl: Changelog, summary: List[str],
        maintainer: Optional[Tuple[str, str]] = None,
        timestamp: Optional[datetime] = None,
        urgency: str = 'low') -> None:
    """Add an entry to a changelog.
    """
    if timestamp is None:
        timestamp = datetime.now()
    if maintainer is None:
        maintainer = get_maintainer()
    if (cl[0].distributions == 'UNRELEASED' or (
            cl[0].author is None and cl[0].date is None)):
        by_author = list(changes_by_author(cl[0].changes()))
        if all([author is None for (author, linenos, change) in by_author]):
            if cl[0].author is not None:
                entry_maintainer = parseaddr(cl[0].author)
                if entry_maintainer != maintainer:
                    cl[0]._changes.insert(1, '  [ %s ]' % entry_maintainer[0])
                    if cl[0]._changes[-1]:
                        cl[0]._changes.append('')
                    cl[0]._changes.append('  [ %s ]' % maintainer[0])
        else:
            if by_author[-1][0] != maintainer[0]:
                if cl[0]._changes[-1]:
                    cl[0]._changes.append('')
                cl[0]._changes.append('  [ %s ]' % maintainer[0])
        if not cl[0]._changes[-1].strip():
            del cl[0]._changes[-1]
    else:
        cl.new_block(
            package=cl[0].package,
            version=_inc_version(cl[0].version),
            urgency=urgency,
            author="%s <%s>" % maintainer,
            date=format_datetime(timestamp),
            distributions='UNRELEASED',
            changes=[''])
    wrapper = TextWrapper(INITIAL_INDENT)
    cl[0]._changes.extend(wrapper.wrap(summary[0]))
    for line in summary[1:]:
        prefix = len(INITIAL_INDENT) * ' '
        m = re.match(r'^[  ]*[\+\-\*] ', line)
        if m:
            prefix += m.group(0)
            line = line[len(m.group(0)):]
        cl[0]._changes.extend(TextWrapper(prefix).wrap(line))
    cl[0]._changes.append('')

