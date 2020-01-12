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

"""Utility functions for dealing with lintian overrides files."""

import collections
import os


# https://lintian.debian.org/manual/section-2.4.html
# File format (as documented in policy 2.4.1):
# [[<package>][ <archlist>][ <type>]: ]<lintian-tag>[ [*]<lintian-info>[*]]


VALID_TYPES = ['udeb', 'source', 'binary']
Override = collections.namedtuple(
    'Override', ['package', 'archlist', 'type', 'tag', 'info'])
Override.__new__.__defaults__ = (None,) * len(Override._fields)


def overrides_paths():
    for path in ['debian/source/lintian-overrides']:
        if os.path.exists(path):
            yield path


def update_overrides(cb):
    """"Call update_overrides_file on all overrides files.

    Args:
      cb: Callback that modifies overrides; called with an Override object
    """
    for path in overrides_paths():
        update_overrides_file(cb, path=path)


def parse_override(line):
    """Parse an override line

    Args:
      line: Line to parse
    Returns:
      An Override object
    Raises:
      ValueError: when encountering invalid syntax
    """
    line = line.strip()
    archlist = None
    package = None
    type = None
    if ': ' in line:
        origin, issue = line.split(': ', 1)
        while origin:
            origin = origin.strip()
            if origin.startswith('['):
                archlist, origin = origin[1:].split(']', 1)
            else:
                try:
                    field, origin = origin.split(' ', 1)
                except ValueError:
                    field = origin
                    origin = ''
                if field in VALID_TYPES:
                    type = field
                else:
                    package = field
    else:
        issue = line
    try:
        tag, info = issue.split(None, 1)
    except ValueError:
        tag = issue
        info = None
    return Override(
        package=package, archlist=archlist, type=type, tag=tag, info=info)


def serialize_override(override):
    """Serialize an override.

    Args:
      override: An Override object
    Returns:
      serialized override, including newline
    """
    origin = []
    if override.package:
        origin.append(override.package)
    if override.archlist:
        origin.append('[' + override.archlist + ']')
    if override.type:
        origin.append(override.type)
    if origin:
        line = ' '.join(origin) + ': ' + override.tag
    else:
        line = override.tag
    if override.info:
        line += ' ' + override.info
    return line + '\n'


def update_overrides_file(cb, path='debian/source/lintian-overrides'):
    """Modify the overrides in a file.

    Args:
      cb: Callback that modifies overrides; called with an Override object
        Should return new override or None to delete override.
    Returns:
        Whether the file was modified
    """
    changed = False
    lines = []
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith('#') or not line.strip():
                lines.append(line)
            else:
                old = parse_override(line)
                new = cb(old)
                if old != new:
                    changed = True
                    if new is not None:
                        lines.append(serialize_override(new))
                else:
                    lines.append(line)

    if not changed:
        return False

    if lines:
        with open(path, 'w') as f:
            f.writelines(lines)
    else:
        os.unlink(path)

    return True


def _get_overrides(package=None):
    if package in ('source', None):
        paths = ['debian/source/lintian-overrides',
                 'debian/source.lintian-overrides']
    else:
        paths = []
        # TODO(jelmer)

    for path in paths:
        try:
            with open(path, 'r') as f:
                for line in f.readlines():
                    if line.startswith('#') or not line.strip():
                        pass
                    else:
                        yield parse_override(line)
        except FileNotFoundError:
            pass


def override_exists(tag, info=None, package=None):
    """Check if a particular override exists.

    Args:
      tag: Tag name
      info: Optional info
      package: Package (as type, name tuple)
    """
    for override in _get_overrides(package):
        if override.tag != tag:
            continue
        if override.info and info != override.info:
            continue
        return True
    return False


async def get_unused_overrides(packages):
    from .udd import connect_udd_mirror
    udd = await connect_udd_mirror()

    args = []
    extra = []
    for (type, name) in packages:
        extra.append('package = $%d AND package_type = $%d' % (
            len(args)+1, len(args)+2))
        args.extend([name, type])

    return list(await udd.fetch(
        """\
select package, package_type, package_version, information
from lintian where tag = 'unused-override' AND (%s)""" % " OR ".join(extra),
        *args))


async def remove_unused():
    from debian.deb822 import Deb822
    packages = []
    with open('debian/control', 'r') as f:
        for para in Deb822.iter_paragraphs(f):
            if 'Source' in para:
                packages.append(('source', para['Source']))
            else:
                packages.append(('binary', para['Package']))
    unused_overrides = await get_unused_overrides(packages)
    removed = []

    def drop_override(override):
        for unused_override in unused_overrides:
            if override.package not in (None, unused_override[0]):
                continue
            if override.type not in (None, unused_override[1]):
                continue
            if override.info:
                expected_info = '%s %s' % (override.tag, override.info)
            else:
                expected_info = override.tag
            if expected_info != unused_override[3]:
                continue
            removed.append(override)
            return None
        return override
    update_overrides(drop_override)
    return removed


if __name__ == '__main__':
    import argparse
    import asyncio
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '--remove-unused', action='store_true',
        help='Remove unused overrides.')
    args = parser.parse_args()
    if args.remove_unused:
        removed = asyncio.run(remove_unused())
        print('Removed %d unused overrides' % len(removed))
    else:
        parser.print_usage()
