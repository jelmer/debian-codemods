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
    check_generated_file,
    edit_formatted_file,
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


def update_deb822(path, **kwargs):
    """Update a deb822-style file.

    The callbacks can modify the paragraphs in place, and can trigger their
    removal by clearing the paragraph.

    Args:
      path: Path to the debian/control file to edit
      source_package_cb: Called on source package paragraph
      binary_package_cb: Called on each binary package paragraph
    Returns:
      boolean indicating whether any changes were made
    """
    check_generated_file(path)
    with open(path, 'rb') as f:
        original_contents = f.read()
    rewritten_contents = reformat_deb822(original_contents)
    outf = BytesIO()
    update_control_file(BytesIO(original_contents), outf, **kwargs)
    updated_contents = outf.getvalue()
    return edit_formatted_file(
        path, original_contents, rewritten_contents, updated_contents)


def update_control_file(inf, outf, source_package_cb=None,
                        binary_package_cb=None):
    """Update a control file.

    The callbacks can modify the paragraphs in place, and can trigger their
    removal by clearing the paragraph.

    Args:
      inf: File-like object to read control file from
      outf: File-like object to write control file to
      source_package_cb: Called on source package paragraph (optional)
      binary_package_cb: Called on each binary package paragraph (optional)
    """
    paragraphs = []
    for paragraph in Deb822.iter_paragraphs(inf, encoding='utf-8'):
        if paragraph.get("Source"):
            if source_package_cb is not None:
                source_package_cb(paragraph)
        else:
            if binary_package_cb is not None:
                binary_package_cb(paragraph)
        paragraphs.append(paragraph)
    outf.write(dump_paragraphs(paragraphs))
