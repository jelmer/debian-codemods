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

from debian.changelog import Version
from debian.deb822 import Deb822

from ._deb822 import PkgRelation


class GeneratedFile(Exception):
    """File is generated and should not be edited."""


class FormattingUnpreservable(Exception):
    """Formatting unpreservable."""


def can_preserve_deb822(contents):
    """Check whether it's possible to preserve a control file.

    Args:
      contents: Original contents
    Returns:
      New contents
    """
    outf = BytesIO()
    for paragraph in Deb822.iter_paragraphs(BytesIO(contents),
                                            encoding='utf-8'):
        paragraph.dump(fd=outf, encoding='utf-8')
        outf.write(b'\n')
    return outf.getvalue().strip() == contents.strip()


def update_control(path='debian/control', **kwargs):
    """Update a control file.

    The callbacks can modify the paragraphs in place, and can trigger their
    removal by clearing the paragraph.

    Args:
      path: Path to the debian/control file to edit
      source_package_cb: Called on source package paragraph
      binary_package_cb: Called on each binary package paragraph
    """
    with open(path, 'rb') as f:
        original_contents = f.read()
    if b"DO NOT EDIT" in original_contents:
        raise GeneratedFile()
    if not can_preserve_deb822(original_contents):
        raise FormattingUnpreservable("Unable to preserve formatting", path)
    outf = BytesIO()
    update_control_file(BytesIO(original_contents), outf, **kwargs)
    updated_contents = outf.getvalue()
    if updated_contents.strip() != original_contents.strip():
        with open(path, 'wb') as f:
            f.write(updated_contents)


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


def parse_relations(text):
    """Parse a package relations string.

    (e.g. a Depends, Provides, Build-Depends, etc field)

    This attemps to preserve some indentation.

    Args:
      text: Text to parse
    Returns:
    """
    ret = []
    for top_level in text.split(','):
        if top_level == "":
            if ',' not in text:
                return []
        if top_level.isspace():
            ret.append((top_level, [], ''))
            continue
        head_whitespace = ''
        for i in range(len(top_level)):
            if not top_level[i].isspace():
                if i > 0:
                    head_whitespace = top_level[:i]
                top_level = top_level[i:]
                break
        tail_whitespace = ''
        for i in range(len(top_level)):
            if not top_level[-(i+1)].isspace():
                if i > 0:
                    tail_whitespace = top_level[-i:]
                    top_level = top_level[:-i]
                break
        ret.append((head_whitespace, PkgRelation.parse(top_level),
                    tail_whitespace))
    return ret


def format_relations(relations):
    """Format a package relations string.

    This attemps to create formatting.
    """
    ret = []
    for (head_whitespace, relation, tail_whitespace) in relations:
        ret.append(head_whitespace + ' | '.join(o.str() for o in relation) +
                   tail_whitespace)
    return ','.join(ret)


def ensure_minimum_version(relationstr, package, minimum_version):
    """Update a relation string to ensure a particular version is required.

    Args:
      relationstr: package relation string
      package: package name
      minimum_version: Minimum version
    Returns:
      updated relation string
    """
    minimum_version = Version(minimum_version)
    found = False
    changed = False
    relations = parse_relations(relationstr)
    for (head_whitespace, relation, tail_whitespace) in relations:
        if isinstance(relation, str):  # formatting
            continue
        names = [r.name for r in relation]
        if len(names) > 1 and names[0] == package:
            raise Exception("Complex rule for %s , aborting" % package)
        if names != [package]:
            continue
        found = True
        if (relation[0].version is None or
                Version(relation[0].version[1]) < minimum_version):
            relation[0].version = ('>=', minimum_version)
            changed = True
    if not found:
        changed = True
        relations.append(
            (' ' if len(relations) > 0 else '',
                [PkgRelation(name=package, version=('>=', minimum_version))],
                ''))
    if changed:
        return format_relations(relations)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def drop_dependency(relationstr, package):
    """Drop a dependency from a depends line.

    Args:
      relationstr: package relation string
      package: package name
    Returns:
      updated relation string
    """
    relations = parse_relations(relationstr)
    ret = []
    for entry in relations:
        (head_whitespace, relation, tail_whitespace) = entry
        if isinstance(relation, str):  # formatting
            ret.append(entry)
            continue
        names = [r.name for r in relation]
        if set(names) != set([package]):
            ret.append(entry)
            continue
    if relations != ret:
        return format_relations(ret)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr
