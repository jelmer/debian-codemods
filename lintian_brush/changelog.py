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

from datetime import datetime

from typing import Optional, Tuple, List

from breezy.mutabletree import MutableTree

from debmutate.changelog import (
    Changelog,
    changelog_add_entry as _changelog_add_entry,
    )


def add_changelog_entry(
        tree: MutableTree, path: str, summary: List[str],
        maintainer: Optional[Tuple[str, str]] = None,
        timestamp: Optional[datetime] = None,
        urgency: str = 'low') -> None:
    """Add a changelog entry.

    Args:
      tree: Tree to edit
      path: Path to the changelog file
      summary: Entry to add
      maintainer: Maintainer details; tuple of fullname and email
      suppress_warnings: Whether to suppress any warnings from 'dch'
    """
    # TODO(jelmer): This logic should ideally be in python-debian.
    with tree.get_file(path) as f:
        cl = Changelog()
        cl.parse_changelog(
            f, max_blocks=None, allow_empty_author=True, strict=False)
        _changelog_add_entry(
            cl, summary=summary, maintainer=maintainer,
            timestamp=timestamp, urgency=urgency)
    # Workaround until
    # https://salsa.debian.org/python-debian-team/python-debian/-/merge_requests/22
    # lands.
    pieces = []
    for line in cl.initial_blank_lines:
        pieces.append(line.encode(cl._encoding) + b'\n')
    for block in cl._blocks:
        pieces.append(bytes(block))
    tree.put_file_bytes_non_atomic(path, b''.join(pieces))
