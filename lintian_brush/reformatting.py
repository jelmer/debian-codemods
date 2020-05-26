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

__all__ = [
    'check_preserve_formatting',
    'check_generated_file',
    'edit_formatted_file',
    'Updater',
    ]


import os


class GeneratedFile(Exception):
    """The specified file is generated."""

    def __init__(self, path: str, template_path=None, template_type=None):
        self.path = path
        self.template_path = template_path
        self.template_type = template_type


class FormattingUnpreservable(Exception):
    """The file is unpreservable."""

    def __init__(self, path, original_contents, rewritten_contents):
        super(FormattingUnpreservable, self).__init__(path)
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
    Raises:
      GeneratedFile: when a generated file is found
    """
    for ext in ['.in', '.m4']:
        if os.path.exists(path + ext):
            raise GeneratedFile(path, path + ext)
    DO_NOT_EDIT_SCAN_LINES = 20
    try:
        with open(path, 'rb') as f:
            for i, line in enumerate(f):
                if i > DO_NOT_EDIT_SCAN_LINES:
                    break
                if b"DO NOT EDIT" in line:
                    raise GeneratedFile(path)
    except FileNotFoundError:
        return


def edit_formatted_file(
        path, original_contents, rewritten_contents,
        updated_contents, allow_generated=False):
    """Edit a formatted file.

    Args:
      path: path to the file
      original_contents: The original contents of the file
      rewritten_contents: The contents rewritten with our parser/serializer
      updated_contents: Updated contents rewritten with our parser/serializer
        after changes were made.
      allow_generated: Do not raise GeneratedFile when encountering a generated
        file
    """
    if type(updated_contents) != type(rewritten_contents):
        raise TypeError('inconsistent types: %r, %r' % (
            type(updated_contents), type(rewritten_contents)))
    if updated_contents in (rewritten_contents, original_contents):
        return False
    if not allow_generated:
        check_generated_file(path)
    check_preserve_formatting(
            rewritten_contents.strip(),
            original_contents.strip(), path)
    mode = 'w' + ('b' if isinstance(updated_contents, bytes) else '')
    with open(path, mode) as f:
        f.write(updated_contents)
    return True


class Updater(object):
    """Context object for updating a file, preserving formatting."""

    def __init__(self, path: str, mode='', allow_generated=False):
        self.path = path
        self.mode = mode
        self.allow_generated = allow_generated

    def _nonexistant(self):
        raise

    def _parse(self, content):
        """Parse the specified bytestring and returned parsed object."""
        raise NotImplementedError(self._parse)

    def _format(self, parsed):
        """Serialize the parsed object."""
        raise NotImplementedError(self._format)

    def __enter__(self):
        try:
            with open(self.path, 'r' + self.mode) as f:
                self._orig_content = f.read()
        except FileNotFoundError:
            self._parsed = self._nonexistant()
        else:
            self._parsed = self._parse(self._orig_content)
        if self._parsed is not None:
            self._rewritten_content = self._format(self._parsed)
        else:
            self._rewritten_content = None
        return self

    def _updated_content(self):
        if self._parsed is not None:
            return self._format(self._parsed)
        else:
            return None

    def has_changed(self):
        return self._updated_content() not in (
            self._rewritten_content, self._orig_content)

    def __exit__(self, exc_type, exc_val, exc_tb):
        updated_content = self._updated_content()

        if updated_content is None:
            if os.path.exists(self.path):
                os.unlink(self.path)
        else:
            self.changed = edit_formatted_file(
                self.path, self._orig_content, self._rewritten_content,
                updated_content, self.allow_generated)
        return False
