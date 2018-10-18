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

"""Utility functions for dealing with control files."""

from io import BytesIO
from debian.deb822 import Deb822
import sys


class GeneratedFile(Exception):
    """File is generated and should not be edited."""


def update_control(path='debian/control', **kwargs):
    """Update a control file.

    The callbacks can modify the paragraphs in place, and can trigger their removal
    by clearing the paragraph.

    Args:
      path: Path to the debian/control file to edit
      source_package_cb: Called on source package paragraph
      binary_package_cb: Called on each binary package paragraph
    """
    outf = BytesIO()
    with open(path, 'rb') as f:
        original_contents = f.read()
    if b"DO NOT EDIT" in original_contents:
        raise GeneratedFile()
    update_control_file(BytesIO(original_contents), outf, **kwargs)
    updated_contents = outf.getvalue()
    if updated_contents.strip() != original_contents.strip():
        with open(path, 'wb') as f:
            f.write(updated_contents)


def update_control_file(inf, outf, source_package_cb=None, binary_package_cb=None):
    """Update a control file.

    The callbacks can modify the paragraphs in place, and can trigger their removal
    by clearing the paragraph.

    Args:
      inf: File-like object to read control file from
      outf: File-like object to write control file to
      source_package_cb: Called on source package paragraph (optional)
      binary_package_cb: Called on each binary package paragraph (optional)
    """
    first = True
    for paragraph in Deb822.iter_paragraphs(inf, encoding='utf-8'):
        if paragraph.get("Source"):
            if source_package_cb is not None:
                source_package_cb(paragraph)
        else:
            if binary_package_cb is not None:
                binary_package_cb(paragraph)
        if paragraph:
            if not first:
                outf.write(b'\n')
            paragraph.dump(fd=outf, encoding='utf-8')
            first = False
