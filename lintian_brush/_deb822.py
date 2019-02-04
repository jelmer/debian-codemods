#!/usr/bin/python3
# PkgRelation from python-debian, lib/debian/deb822.py (GPL-2+).
# Local changes to add support for preserving whitespace
# Copyright (C) 2005-2006  dann frazier <dannf@dannf.org>
# Copyright (C) 2006-2010  John Wright <john@johnwright.org>
# Copyright (C) 2006       Adeodato Simó <dato@net.com.org.es>
# Copyright (C) 2008       Stefano Zacchiroli <zack@upsilon.cc>
# Copyright (C) 2014       Google, Inc.
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

# TODO(jelmer): Contribute improvements back to python-debian

import collections
import re
import warnings


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
        def parse_archs(raw):
            # type: (str) -> list[PkgRelation.ArchRestriction]
            # assumption: no space between '!' and architecture name
            archs = []
            for arch in cls.__blank_sep_RE.split(raw.strip()):
                disabled = arch[0] == '!'
                if disabled:
                    arch = arch[1:]
                archs.append(cls.ArchRestriction(not disabled, arch))
            return archs

        def parse_restrictions(raw):
            # type: (str) -> list[list[PkgRelation.BuildRestriction]]
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
            match = cls.__dep_RE.match(raw)
            if match:
                parts = match.groupdict()
                d = {
                    'name': parts['name'],
                    'archqual': parts['archqual'],
                    'version': None,
                    'arch': None,
                    'restrictions': None,
                }
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
        if text == "":
            return []
        or_deps = cls.__pipe_sep_RE.split(text)
        return [parse_rel(or_dep) for or_dep in or_deps]

    def __repr__(self):
        return "%s(%r, %r, %r, %r, %r)" % (
                self.__class__.__name__, self.name, self.version, self.arch,
                self.archqual, self.restrictions)

    def __tuple__(self):
        return (self.name, self.version, self.arch, self.archqual,
                self.restrictions)

    def __eq__(self, other):
        if not isinstance(other, PkgRelation):
            return False
        return (self.__tuple__() == other.__tuple__())

    def __lt__(self, other):
        if not isinstance(other, PkgRelation):
            raise TypeError
        return (self.__tuple__() < other.__tuple__())

    def str(self):
        """Format to string structured inter-package relationships

        Perform the inverse operation of parse_relations, returning a string
        suitable to be written in a package stanza.
        """
        def pp_arch(arch_spec):
            # type: (PkgRelation.ArchRestriction) -> str
            return '%s%s' % (
                '' if arch_spec.enabled else '!',
                arch_spec.arch,
            )

        def pp_restrictions(restrictions):
            # type: (list[PkgRelation.BuildRestriction]) -> str
            s = []
            for term in restrictions:
                s.append(
                    '%s%s' % (
                        '' if term.enabled else '!',
                        term.profile
                    )
                )
            return '<%s>' % ' '.join(s)

        s = self.name
        if self.archqual is not None:
            s += ':%s' % self.archqual
        if self.version is not None:
            s += ' (%s %s)' % self.version
        if self.arch is not None:
            s += ' [%s]' % ' '.join(map(pp_arch, self.arch))
        if self.restrictions is not None:
            s += ' %s' % ' '.join(map(pp_restrictions,
                                      self.restrictions))
        return s

    def __init__(self, name, version=None, arch=None, archqual=None,
                 restrictions=None):
        self.name = name
        self.version = version
        self.arch = arch
        self.archqual = archqual
        self.restrictions = restrictions
