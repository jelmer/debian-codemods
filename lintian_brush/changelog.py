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
    'update_changelog',
    ]

from io import StringIO
from debian.changelog import (
    Changelog,
    ChangelogCreateError,
    ChangelogParseError,
    )
from .reformatting import edit_formatted_file


def update_changelog(path='debian/changelog', block_cb=None):
    """Update a changelog file."""
    with open(path, 'r') as f:
        orig_content = f.read()
    cl = Changelog(orig_content)
    f = StringIO()
    cl.write_to_open_file(f)
    rewritten_content = f.getvalue()
    for block in cl:
        if block_cb:
            block_cb(block)
    f = StringIO()
    cl.write_to_open_file(f)
    updated_contents = f.getvalue()
    return edit_formatted_file(
        path, orig_content, rewritten_content, updated_contents)
