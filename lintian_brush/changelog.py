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
from .reformatting import edit_formatted_file


class ChangelogUpdater(object):
    """Update a changelog file.

    This will only write out the changelog file if it has been changed.
    """

    def __init__(self, path='debian/changelog'):
        self.path = path

    def __enter__(self):
        with open(self.path, 'r') as f:
            self._orig_content = f.read()
        self._cl = Changelog(self._orig_content)
        f = StringIO()
        self._cl.write_to_open_file(f)
        self._rewritten_content = f.getvalue()
        return self

    @property
    def changelog(self):
        return self._cl

    def __exit__(self, exc_type, exc_val, exc_tb):
        f = StringIO()
        self._cl.write_to_open_file(f)
        updated_contents = f.getvalue()
        self.changed = edit_formatted_file(
            self.path, self._orig_content,
            self._rewritten_content, updated_contents)
        return False
