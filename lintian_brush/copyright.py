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
    'update_copyright',
    ]

from debian.copyright import (
    Copyright,
    NotMachineReadableError,
    )

from .reformatting import edit_formatted_file


def update_copyright(update_cb, path='debian/copyright'):
    with open(path, 'r') as f:
        orig_content = f.read()

    copyright = Copyright(orig_content, strict=False)
    rewritten_content = copyright.dump()

    update_cb(copyright)

    updated_content = copyright.dump()

    return edit_formatted_file(
        path, orig_content, rewritten_content, updated_content)
