#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij <jelmer@debian.org>
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

__all__ = ['check_preserve_formatting', 'check_generated_file']


import os


class GeneratedFile(Exception):
    """The specified file is generated."""

    def __init__(self, path):
        self.path = path


class FormattingUnpreservable(Exception):
    """The file is unpreservable."""

    def __init__(self, path, original_contents, rewritten_contents):
        self.path = path
        self.original_contents = original_contents
        self.rewritten_contents = rewritten_contents


def check_preserve_formatting(rewritten_text, text, path):
    """Check that formatting can be preserved.

    If the REFORMATTING environment variable is set to 'allow',
    then reformatting will be allowed.

    Args:
      rewritten_text: The rewritten file contents
      text: The original file contents
      path: Path to the file (unused, just passed to the exception)
    Raises:
      FormattingUnpreservable: Raised when formatting could not be preserved
    """
    if rewritten_text == text:
        return
    if os.environ.get('REFORMATTING', 'disallow') == 'allow':
        return
    raise FormattingUnpreservable(path, text, rewritten_text)


def check_generated_file(path):
    """Check if a file is generated from another file.

    Args:
      path: Path to the file to check
    """
    if os.path.exists(path + '.in'):
        raise GeneratedFile(path)
    try:
        with open(path, 'rb') as f:
            original_contents = f.read()
    except FileNotFoundError:
        return
    if b"DO NOT EDIT" in original_contents:
        raise GeneratedFile(path)
