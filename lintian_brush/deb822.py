#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij
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

"""Utility functions for dealing with deb822 files."""

from debian.deb822 import Deb822
from io import BytesIO
from .reformatting import (
    Updater,
    )


def dump_paragraphs(paragraphs):
    """Dump a set of deb822 paragraphs to a file.

    Args:
      paragraphs: iterable over paragraphs
    Returns:
      formatted text (as bytes)
    """
    outf = BytesIO()
    first = True
    for paragraph in paragraphs:
        if paragraph:
            if not first:
                outf.write(b'\n')
            paragraph.dump(fd=outf, encoding='utf-8')
            first = False
    return outf.getvalue()


def reformat_deb822(contents):
    """Check whether it's possible to preserve a control file.

    Args:
      contents: Original contents
    Returns:
      New contents
    """
    return dump_paragraphs(
        Deb822.iter_paragraphs(BytesIO(contents), encoding='utf-8'))


class Deb822Updater(Updater):

    def __init__(self, path, allow_generated=False):
        super(Deb822Updater, self).__init__(
            path, allow_generated=allow_generated, mode='b')

    def apply_changes(self, changes):
        """Apply a set of changes to this deb822 instance.

        Args:
          dict mapping paragraph types to
          list of (field_name, old_value, new_value)
        """
        changes = dict(changes.items())
        for paragraph in self.paragraphs:
            for item in paragraph.items():
                for key, old_value, new_value in changes.pop(item, []):
                    if new_value is None:
                        del paragraph[key]
                    else:
                        paragraph[key] = new_value
        # Add any new paragraphs that weren't processed earlier
        for p in changes.values():
            self.paragraphs.append(Deb822(
                [(field, new_value)
                 for (field, old_value, new_value) in p]))

    def _parse(self, content):
        return list(Deb822.iter_paragraphs(content, encoding='utf-8'))

    @property
    def paragraphs(self):
        return self._parsed

    def _format(self, paragraphs):
        return dump_paragraphs(paragraphs)
