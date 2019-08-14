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

from debian.changelog import Version

from ._deb822 import PkgRelation
from .deb822 import update_deb822


def update_control(path='debian/control', source_package_cb=None,
                   binary_package_cb=None):
    def paragraph_cb(paragraph):
        if paragraph.get("Source"):
            if source_package_cb is not None:
                source_package_cb(paragraph)
        else:
            if binary_package_cb is not None:
                binary_package_cb(paragraph)

    return update_deb822(path, paragraph_cb=paragraph_cb)


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


def get_relation(relationstr, package):
    """Retrieve the relation for a particular package.

    Args:
      relationstr: package relation string
      package: package name
    Returns:
      Relation object
    """
    relations = parse_relations(relationstr)
    for (head_whitespace, relation, tail_whitespace) in relations:
        if isinstance(relation, str):  # formatting
            continue
        names = [r.name for r in relation]
        if len(names) > 1 and names[0] == package:
            raise Exception("Complex rule for %s , aborting" % package)
        if names != [package]:
            continue
        return relation
    raise KeyError(package)


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
        _add_dependency(
            relations,
            [PkgRelation(name=package, version=('>=', minimum_version))])
    if changed:
        return format_relations(relations)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def ensure_exact_version(relationstr, package, version):
    """Update a relation string to depend on a specific version.

    Args:
      relationstr: package relation string
      package: package name
      version: Exact version to depend on
    Returns:
      updated relation string
    """
    version = Version(version)
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
                (relation[0].version[0],
                 Version(relation[0].version[1])) != ('=', version)):
            relation[0].version = ('=', version)
            changed = True
    if not found:
        changed = True
        _add_dependency(
            relations,
            [PkgRelation(name=package, version=('=', version))])
    if changed:
        return format_relations(relations)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def _add_dependency(relations, relation):
    """Add a dependency to a depends line.

    Args:
      relations: existing list of relations
      relation: New relation
    Returns:
      Nothing
    """
    if len(relations) == 0:
        head_whitespace = ''
    elif len(relations) == 1:
        head_whitespace = (relations[0][0] or " ")  # Best guess
    else:
        ws = collections.defaultdict(lambda: 0)
        for r in relations[1:]:
            ws[r[0]] += 1
        if len(ws) == 1:
            head_whitespace = list(ws.keys())[0]
        else:
            head_whitespace = relations[-1][0]  # Best guest
    if len(relations) == 0:
        tail_whitespace = ''
    else:
        tail_whitespace = relations[-1][2]
        relations[-1] = relations[-1][:2] + ('', )

    relations.append((head_whitespace, relation, tail_whitespace))


def add_dependency(relationstr, relation):
    """Add a dependency to a depends line.

    Args:
      relationstr: existing relations line
      relation: New relation
    Returns:
      Nothing
    """
    relations = parse_relations(relationstr)
    if isinstance(relation, str):
        relation = PkgRelation.parse(relation)
    _add_dependency(relations, relation)
    return format_relations(relations)


def ensure_some_version(relationstr, package):
    """Add a package dependency to a depends line if it's not there.

    Args:
      relationstr: existing relations line
      package: Package to add dependency on
    Returns:
      new formatted relation string
    """
    if not isinstance(package, str):
        raise TypeError(package)
    relations = parse_relations(relationstr)
    for (head_whitespace, relation, tail_whitespace) in relations:
        if isinstance(relation, str):  # formatting
            continue
        names = [r.name for r in relation]
        if len(names) > 1 and names[0] == package:
            raise Exception("Complex rule for %s , aborting" % package)
        if names != [package]:
            continue
        return relationstr
    _add_dependency(relations, PkgRelation.parse(package))
    return format_relations(relations)


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
    for i, entry in enumerate(relations):
        (head_whitespace, relation, tail_whitespace) = entry
        if isinstance(relation, str):  # formatting
            ret.append(entry)
            continue
        names = [r.name for r in relation]
        if set(names) != set([package]):
            ret.append(entry)
            continue
        elif i == 0 and len(relations) > 1:
            # If the first item is removed, then copy the spacing to the next
            # item
            relations[1] = (head_whitespace, relations[1][1], tail_whitespace)
    if relations != ret:
        return format_relations(ret)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def ensure_minimum_debhelper_version(build_depends, minimum_version):
    """Ensure that the pakcage is at least using version x of debhelper.

    This is a dedicated helper, since debhelper can now also be pulled in
    with a debhelper-compat dependency.

    Args:
      build_depends: Build depends relation
      version: The minimum version
    """
    minimum_version = Version(minimum_version)
    try:
        debhelper_compat = get_relation(
            build_depends, "debhelper-compat")
    except KeyError:
        pass
    else:
        if len(debhelper_compat) > 1:
            raise Exception("Complex rule for debhelper-compat, aborting")
        if debhelper_compat[0].version[0] != '=':
            raise Exception("Complex rule for debhelper-compat, aborting")
        if Version(debhelper_compat[0].version[1]) >= minimum_version:
            return build_depends
    return ensure_minimum_version(
            build_depends,
            "debhelper", minimum_version)
