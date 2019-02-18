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

"""Utility functions for dealing with copyright files."""

__all__ = [
    'NotMachineReadableError',
    'FormattingUnpreservable',
    'update_copyright',
    ]

from debian.copyright import (
    Copyright,
    NotMachineReadableError,
    )


class FormattingUnpreservable(Exception):
    """Formatting unpreservable."""


def update_copyright(update_cb):
    with open('debian/copyright', 'r') as f:
        orig_content = f.read()

    copyright = Copyright(orig_content)
    if copyright.dump() != orig_content:
        raise FormattingUnpreservable(
            "Unable to preserve formatting of debian/copyright")

    update_cb(copyright)

    new_content = copyright.dump()

    if new_content != orig_content:
        with open('debian/copyright', 'w') as f:
            f.write(new_content)
        return True
    return False
