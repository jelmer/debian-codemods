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

import collections
from io import BytesIO
import re
import warnings
from debian.deb822 import Deb822


class GeneratedFile(Exception):
    """File is generated and should not be edited."""


def update_control(path='debian/control', **kwargs):
    """Update a control file.

    The callbacks can modify the paragraphs in place, and can trigger their
    removal by clearing the paragraph.

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


# TODO(jelmer): Contribute improvements back to python-debian
class PkgRelation(object):
    """A package requirement."""

    __dep_RE = re.compile(
        r'^\s*(?P<name>[a-zA-Z0-9.+\-]{2,})'
        r'(:(?P<archqual>([a-zA-Z0-9][a-zA-Z0-9-]*)))?'
        r'(\s*\(\s*(?P<relop>[>=<]+)\s*'
        r'(?P<version>[0-9a-zA-Z:\-+~.]+)\s*\))?'
        r'(\s*\[(?P<archs>[\s!\w\-]+)\])?\s*'
        r'((?P<restrictions><.+>))?\s*'
        r'$')
    __pipe_sep_RE = re.compile(r'\s*\|\s*')
    __blank_sep_RE = re.compile(r'\s+')
    __restriction_sep_RE = re.compile(r'>\s*<')
    __restriction_RE = re.compile(
        r'(?P<enabled>\!)?'
        r'(?P<profile>[^\s]+)')

    ArchRestriction = collections.namedtuple('ArchRestriction',
                                             ['enabled', 'arch'])
    BuildRestriction = collections.namedtuple('BuildRestriction',
                                              ['enabled', 'profile'])

    @classmethod
    def parse(cls, text):
        # From python-debian, deb822.py (GPL-2+)
        def parse_archs(raw):
            # type: (str) -> List[PkgRelation.ArchRestriction]
            # assumption: no space between '!' and architecture name
            archs = []
            for arch in cls.__blank_sep_RE.split(raw.strip()):
                disabled = arch[0] == '!'
                if disabled:
                    arch = arch[1:]
                archs.append(cls.ArchRestriction(not disabled, arch))
            return archs

        def parse_restrictions(raw):
            # type: (str) -> List[List[PkgRelation.BuildRestriction]]
            """ split a restriction formula into a list of restriction lists

            Each term in the restriction list is a namedtuple of form:

                (enabled, label)

            where
                enabled: bool: whether the restriction is positive or negative
                profile: the profile name of the term e.g. 'stage1'
            """
            restrictions = []
            groups = cls.__restriction_sep_RE.split(raw.lower().strip('<> '))
            for rgrp in groups:
                group = []
                for restriction in cls.__blank_sep_RE.split(rgrp):
                    match = cls.__restriction_RE.match(restriction)
                    if match:
                        parts = match.groupdict()
                        group.append(
                            cls.BuildRestriction(
                                parts['enabled'] != '!',
                                parts['profile'],
                            ))
                restrictions.append(group)
            return restrictions

        def parse_rel(raw):
            # type: (str) -> Dict[str, Optional[Union[str, list, Tuple[str, str]]]]
            match = cls.__dep_RE.match(raw)
            if match:
                parts = match.groupdict()
                d = {
                    'name': parts['name'],
                    'archqual': parts['archqual'],
                    'version': None,
                    'arch': None,
                    'restrictions': None,
                }  # type: Dict[str, Optional[Union[str, list, Tuple[str, str]]]]
                if parts['relop'] or parts['version']:
                    d['version'] = (parts['relop'], parts['version'])
                if parts['archs']:
                    d['arch'] = parse_archs(parts['archs'])
                if parts['restrictions']:
                    d['restrictions'] = parse_restrictions(
                        parts['restrictions'])
                return PkgRelation(**d)

            warnings.warn(
                'cannot parse package'
                ' relationship "%s", returning it raw' % raw)
            return PkgRelation(
                name=raw,
                version=None,
                arch=None
                )

        or_deps = cls.__pipe_sep_RE.split(text)
        return [parse_rel(or_dep) for or_dep in or_deps]

    def __repr__(self):
        return "%s(%r, %r, %r, %r, %r)" % (self.__class__.__name__,
                self.name, self.version, self.arch, self.archqual, self.restrictions)

    def __tuple__(self):
        return (self.name, self.version, self.arch, self.archqual, self.restrictions)

    def __eq__(self, other):
        if not isinstance(other, PkgRelation):
            return False
        return (self.__tuple__() == other.__tuple__())

    def __lt__(self, other):
        if not isinstance(other, PkgRelation):
            raise TypeError
        return (self.__tuple__() < other.__tuple__())

    def __init__(self, name, version=None, arch=None, archqual=None, restrictions=None):
        self.name = name
        self.version = version
        self.arch = arch
        self.archqual = archqual
        self.restrictions = restrictions


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
        for i in range(len(top_level)):
            if not top_level[i].isspace():
                if i > 0:
                    ret.append(top_level[:i])
                top_level = top_level[i:]
                break
        tail_whitespace = None
        for i in range(len(top_level)):
            if not top_level[-(i+1)].isspace():
                if i > 0:
                    tail_whitespace = top_level[-i:]
                    top_level = top_level[:-i]
                break
        ret.append(PkgRelation.parse(top_level))
        if tail_whitespace is not None:
            ret.append(tail_whitespace)
    return ret
