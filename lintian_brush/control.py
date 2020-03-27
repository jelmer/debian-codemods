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
from itertools import takewhile
import os

from debian.changelog import Version
from debian.deb822 import Deb822
import subprocess

from ._deb822 import PkgRelation
from .deb822 import Deb822Updater
from .reformatting import GeneratedFile


def dh_gnome_clean(path='.'):
    """Run the dh_gnome_clean command.

    This needs to do some post-hoc cleaning, since dh_gnome_clean
    writes various debhelper log files that should not be checked in.
    """
    for n in os.listdir(os.path.join(path, 'debian')):
        if n.endswith('.debhelper.log'):
            raise AssertionError('pre-existing .debhelper.log files')
    subprocess.check_call(["dh_gnome_clean"], cwd=path)
    for n in os.listdir(os.path.join(path, 'debian')):
        if n.endswith('.debhelper.log'):
            os.unlink(os.path.join(path, 'debian', n))


def pg_buildext_updatecontrol(path='.'):
    """Run the 'pg_buildext updatecontrol' command.
    """
    subprocess.check_call(["pg_buildext", "updatecontrol"], cwd=path)


def guess_template_type(template_path):
    try:
        with open(template_path, 'rb') as f:
            template = f.read()
            if b'@GNOME_TEAM@' in template:
                return 'gnome'
            elif b'@cdbs@' in template:
                return 'cdbs'
            elif b'PGVERSION' in template:
                return 'postgresql'
            elif b'@lintian-brush-test@' in template:
                return 'lintian-brush-test'
            else:
                deb822 = Deb822(template)
                build_depends = deb822.get('Build-Depends', '')
                if any(iter_relations(build_depends, 'gnome-pkg-tools')):
                    return 'gnome'
                if any(iter_relations(build_depends, 'cdbs')):
                    return 'cdbs'
    except IsADirectoryError:
        return 'directory'
    return None


def _update_control_template(template_path, path, changes):
    template_type = guess_template_type(template_path)
    if template_type is None:
        raise GeneratedFile(path, template_path)
    with Deb822Updater(template_path) as updater:
        updater.apply_changes(changes)
    if not updater.changed:
        # A bit odd, since there were changes to the output file. Anyway.
        return False
    package_root = os.path.dirname(os.path.dirname(path)) or '.'
    if template_type == 'cdbs':
        with Deb822Updater(path, allow_generated=True) as updater:
            updater.apply_changes(changes)
    elif template_type == 'gnome':
        dh_gnome_clean(package_root)
    elif template_type == 'postgresql':
        pg_buildext_updatecontrol(package_root)
    elif template_type == 'lintian-brush-test':
        with open(template_path, 'rb') as inf, open(path, 'wb') as outf:
            outf.write(
                inf.read().replace(b'@lintian-brush-test@', b'testvalue'))
    elif template_type == 'directory':
        raise GeneratedFile(path, template_path)
    else:
        raise AssertionError
    return True


def update_control(path='debian/control', source_package_cb=None,
                   binary_package_cb=None):
    def paragraph_cb(paragraph):
        if paragraph.get("Source"):
            if source_package_cb is not None:
                source_package_cb(paragraph)
        else:
            if binary_package_cb is not None:
                old_fields = list(paragraph)
                binary_package_cb(paragraph)
                # Make sure Description stays the last field
                if list(paragraph) != old_fields:
                    if list(old_fields)[-1] == 'Description':
                        paragraph._Deb822Dict__keys.add('Description')
                        paragraph._Deb822Dict__keys.remove('Description')

    with ControlUpdater(path) as updater:
        for paragraph in updater.paragraphs:
            paragraph_cb(paragraph)
    return updater.changed


class ControlUpdater(object):

    def __init__(self, path='debian/control'):
        self.path = path
        self._primary = Deb822Updater(path)

    @property
    def paragraphs(self):
        return self._primary.paragraphs

    @property
    def source(self):
        if not self._primary.paragraphs[0].get('Source'):
            raise ValueError('first paragraph is not Source')
        return self._primary.paragraphs[0]

    @property
    def binaries(self):
        return self._primary.paragraphs[1:]

    def changes(self):
        orig = self._primary._parse(self._primary._orig_content)
        changes = {}

        def by_key(ps):
            ret = {}
            for p in ps:
                if 'Source' in p:
                    ret[('Source', p['Source'])] = p
                else:
                    ret[('Package', p['Package'])] = p
            return ret

        orig_by_key = by_key(orig)
        new_by_key = by_key(self.paragraphs)
        for key in set(orig_by_key).union(set(new_by_key)):
            old = orig_by_key.get(key, {})
            new = new_by_key.get(key, {})
            if old == new:
                continue
            fields = list(old)
            fields.extend([field for field in new if field not in fields])
            for field in fields:
                if old.get(field) != new.get(field):
                    changes.setdefault(key, []).append((field, new.get(field)))
        return changes

    def __enter__(self):
        self._primary.__enter__()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        try:
            self._primary.__exit__(exc_type, exc_val, exc_tb)
        except GeneratedFile as e:
            if not e.template_path:
                raise
            self.changed = _update_control_template(
                e.template_path, self.path, self.changes())
        except FileNotFoundError:
            for template_path in [self.path + '.in', self.path + '.m4']:
                if os.path.exists(template_path):
                    self.changed = _update_control_template(
                        template_path, self.path, self.changes())
            else:
                raise
        else:
            self.changed = self._primary.changed
        return False


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
      Tuple with offset and relation object
    """
    for offset, relation in iter_relations(relationstr, package):
        names = [r.name for r in relation]
        if len(names) > 1 and package in names:
            raise ValueError("Complex rule for %s , aborting" % package)
        if names != [package]:
            continue
        return offset, relation
    raise KeyError(package)


def iter_relations(relationstr, package):
    """Iterate over the relations relevant for a particular package.

    Args:
      relationstr: package relation string
      package: package name
    Yields:
      Tuples with offset and relation objects
    """
    relations = parse_relations(relationstr)
    for i, (head_whitespace, relation, tail_whitespace) in enumerate(
            relations):
        if isinstance(relation, str):  # formatting
            continue
        names = [r.name for r in relation]
        if package not in names:
            continue
        yield i, relation


def ensure_minimum_version(relationstr, package, minimum_version):
    """Update a relation string to ensure a particular version is required.

    Args:
      relationstr: package relation string
      package: package name
      minimum_version: Minimum version
    Returns:
      updated relation string
    """
    def is_obsolete(relation):
        for r in relation:
            if r.name != package:
                continue
            if r.version[0] == '>>' and r.version[1] < minimum_version:
                return True
            if r.version[0] == '>=' and r.version[1] <= minimum_version:
                return True
        return False

    minimum_version = Version(minimum_version)
    found = False
    changed = False
    relations = parse_relations(relationstr)
    obsolete_relations = []
    for i, (head_whitespace, relation, tail_whitespace) in enumerate(
            relations):
        if isinstance(relation, str):  # formatting
            continue
        names = [r.name for r in relation]
        if len(names) > 1 and package in names and is_obsolete(relation):
            obsolete_relations.append(i)
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
    for i in reversed(obsolete_relations):
        del relations[i]
    if changed:
        return format_relations(relations)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def ensure_exact_version(relationstr, package, version, position=None):
    """Update a relation string to depend on a specific version.

    Args:
      relationstr: package relation string
      package: package name
      version: Exact version to depend on
      position: Optional position in the list to insert any new entries
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
            [PkgRelation(name=package, version=('=', version))],
            position=position)
    if changed:
        return format_relations(relations)
    # Just return the original; we don't preserve all formatting yet.
    return relationstr


def _add_dependency(relations, relation, position=None):
    """Add a dependency to a depends line.

    Args:
      relations: existing list of relations
      relation: New relation
      position: Optional position to insert the new relation
    Returns:
      Nothing
    """
    if len(relations) > 0 and not relations[-1][1]:
        pointless_tail = relations.pop(-1)
    else:
        pointless_tail = None
    if len(relations) == 0:
        head_whitespace = ''
        tail_whitespace = ''
    elif len(relations) == 1:
        head_whitespace = (relations[0][0] or " ")  # Best guess
        tail_whitespace = ''
    else:
        hws = collections.defaultdict(lambda: 0)
        for r in relations[1:]:
            hws[r[0]] += 1
        if len(hws) == 1:
            head_whitespace = list(hws.keys())[0]
        else:
            head_whitespace = relations[-1][0]  # Best guess
        tws = collections.defaultdict(lambda: 0)
        for r in relations[0:-1]:
            tws[r[2]] += 1
        if len(tws) == 1:
            tail_whitespace = list(tws.keys())[0]
        else:
            tail_whitespace = relations[0][2]  # Best guess

    if position is None:
        position = len(relations)

    if position < 0 or position > len(relations):
        raise IndexError('position out of bounds: %r' % position)

    if position == len(relations):
        if len(relations) == 0:
            last_tail_whitespace = ''
        else:
            last_tail_whitespace = relations[-1][2]
            relations[-1] = relations[-1][:2] + (tail_whitespace, )
        relations.append((head_whitespace, relation, last_tail_whitespace))
    elif position == 0:
        relations.insert(
            position, (relations[0][0], relation, tail_whitespace))
        relations[1] = (head_whitespace, relations[1][1], relations[1][2])
    else:
        relations.insert(
            position, (head_whitespace, relation, tail_whitespace))
    if pointless_tail:
        relations.append(pointless_tail)


def add_dependency(relationstr, relation, position=None):
    """Add a dependency to a depends line.

    Args:
      relationstr: existing relations line
      relation: New relation
      position: Optional position to insert relation at (defaults to last)
    Returns:
      Nothing
    """
    relations = parse_relations(relationstr)
    if isinstance(relation, str):
        relation = PkgRelation.parse(relation)
    _add_dependency(relations, relation, position=position)
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
        offset, debhelper_compat = get_relation(
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


def delete_from_list(liststr, item_to_delete):
    items = liststr.split(',')
    item_to_delete = item_to_delete.strip()
    if not item_to_delete:
        return liststr
    for i, item in enumerate(items):
        if item.strip() == item_to_delete:
            deleted_item = items.pop(i)
            head_whitespace = ''.join(
                takewhile(lambda x: x.isspace(), deleted_item))
            if i == 0 and len(items) >= 1:
                # If we're removing the first item, copy its whitespace to the
                # second
                items[i] = head_whitespace + items[i].lstrip()
            elif i == len(items):
                if i > 1:
                    items[i-1] = items[i-1].rstrip()
    return ','.join(items)


def read_debian_compat_file(path):
    """Read a debian/compat file.

    Args:
      path: Path to read from
    """
    with open(path, 'r') as f:
        line = f.readline().split('#', 1)[0]
        return int(line.strip())


def get_debhelper_compat_version(path='.'):
    try:
        return read_debian_compat_file(os.path.join(path, 'debian/compat'))
    except FileNotFoundError:
        pass

    try:
        with open(os.path.join(path, 'debian/control'), 'r') as f:
            control = Deb822(f)
    except FileNotFoundError:
        return None

    try:
        offset, [relation] = get_relation(
            control.get("Build-Depends", ""), "debhelper-compat")
    except (IndexError, KeyError):
        return None
    else:
        return int(str(relation.version[1]))


def is_dep_implied(dep, outer):
    """Check if one dependency is implied by another.
    """
    if dep.name != outer.name:
        return False
    if not dep.version:
        return True
    if outer.version == dep.version:
        return True
    if not outer.version:
        return False
    if dep.version[0] == '>=':
        if outer.version[0] == '>>':
            return Version(outer.version[1]) > Version(dep.version[1])
        elif outer.version[0] in ('>=', '='):
            return Version(outer.version[1]) >= Version(dep.version[1])
        elif outer.version[0] in ('<<', '<='):
            return False
        else:
            raise AssertionError('unsupported: %s' % outer.version[0])
    elif dep.version[0] == '=':
        if outer.version[0] == '=':
            return Version(outer.version[1]) == Version(dep.version[1])
        else:
            return False
    else:
        raise AssertionError('unable to handle checks for %s' % dep.version[0])


def is_relation_implied(inner, outer):
    """Check if one relation implies another.

    Args:
      inner: Inner relation
      outer: Outer relation
    Return: boolean
    """
    if isinstance(inner, str):
        inner = PkgRelation.parse(inner)
    if isinstance(outer, str):
        outer = PkgRelation.parse(outer)

    if inner == outer:
        return True

    # "bzr >= 1.3" implied by "bzr >= 1.3 | libc6"
    for inner_dep in inner:
        if all(is_dep_implied(inner_dep, outer_dep) for outer_dep in outer):
            return True
    return False
