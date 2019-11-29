#!/usr/bin/python3
# Copyright (C) 2019 Jelmer Vernooij
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

"""Utility functions for applying multi-arch hints."""

import re

from urllib.request import urlopen, Request

from lintian_brush import (
    USER_AGENT,
    DEFAULT_URLLIB_TIMEOUT,
    add_changelog_entry,
    )
from lintian_brush.control import (
    update_control,
    format_relations,
    parse_relations,
    )


MULTIARCH_HINTS_URL = 'https://dedup.debian.net/static/multiarch-hints.yaml'


def parse_multiarch_hints(f):
    """Parse a multi-arch hints file.

    Args:
      f: File-like object to read from
    Returns:
      dictionary mapping binary package names to lists of hints
    """
    import ruamel.yaml
    import ruamel.yaml.reader
    data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
    if data.get('format') != 'multiarch-hints-1.0':
        raise ValueError('invalid file format: %r' % data.get('format'))
    ret = {}
    for entry in data['hints']:
        ret.setdefault(entry['binary'], []).append(entry)
    return ret


def download_multiarch_hints(url=MULTIARCH_HINTS_URL):
    """Load multi-arch hints from a URL.

    Args:
      url: URL to read from
    Returns:
      parsed multi-arch hints file, see parse_multiarch_hints
    """
    headers = {'User-Agent': USER_AGENT}
    with urlopen(
            Request(url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT) as f:
        return parse_multiarch_hints(f)


def add_message(tree, binary, message):
    add_changelog_entry(
        tree, 'debian/changelog', '%s: %s' % (binary['Package'], message))


def apply_hint_ma_foreign(tree, binary, hint):
    if binary.get('Multi-Arch') != 'foreign':
        binary['Multi-Arch'] = 'foreign'
        return 'Add Multi-Arch: foreign.', 'certain'


def apply_hint_ma_foreign_lib(tree, binary, hint):
    if binary.get('Multi-Arch') == 'foreign':
        del binary['Multi-Arch']
        return 'Drop Multi-Arch: foreign.', 'certain'


def apply_hint_file_conflict(tree, binary, hint):
    if binary.get('Multi-Arch') == 'same':
        del binary['Multi-Arch']
        return 'Drop Multi-Arch: same.', 'certain'


def apply_hint_dep_any(tree, binary, hint):
    m = re.match(
        '(.*) could have its dependency on (.*) annotated with :any',
        hint['description'])
    if not m or m.group(1) != binary['Package']:
        raise ValueError(
            'unable to parse hint description: %r' % hint['description'])
    dep = m.group(2)
    if 'Depends' not in binary:
        return
    changed = False
    relations = parse_relations(binary['Depends'])
    for entry in relations:
        (head_whitespace, relation, tail_whitespace) = entry
        if not isinstance(relation, str):  # formatting
            for r in relation:
                if r.name == dep and r.archqual != 'all':
                    r.archqual = 'all'
                    changed = True
    if not changed:
        return
    binary['Depends'] = format_relations(relations)
    return ('Add :all qualifier for %s dependency.' % dep), 'certain'


def apply_hint_ma_same(tree, binary, hint):
    if binary.get('Multi-Arch') == 'same':
        return
    binary['Multi-Arch'] = 'same'
    return 'Add Multi-Arch: same.', 'certain'


def apply_hint_arch_all(tree, binary, hint):
    if binary['Architecture'] == 'all':
        return
    binary['Architecture'] = 'all'
    return 'Make package Architecture: all.', 'possible'


HINT_APPLIERS = {
    'ma-foreign': apply_hint_ma_foreign,
    'file-conflict': apply_hint_file_conflict,
    'ma-foreign-library': apply_hint_ma_foreign_lib,
    'dep-any': apply_hint_dep_any,
    'ma-same': apply_hint_ma_same,
    'arch-all': apply_hint_arch_all,
}


def apply_multiarch_hints(tree, hints, minimum_certainty=None):

    def update_binary(binary):
        for hint in hints.get(binary['Package'], []):
            kind = hint['link'].rsplit('#', 1)[1]
            ret = HINT_APPLIERS[kind](
                tree, binary, hint, minimum_certainty=minimum_certainty)
            if ret:
                description, certainty = ret
                add_message(tree, binary, description)

    return update_control(
        path=tree.abspath('debian/control'),
        binary_package_cb=update_binary)


if __name__ == '__main__':
    import argparse
    from debian.deb822 import Deb822
    import os
    import sys
    from breezy.workingtree import WorkingTree
    import locale
    locale.setlocale(locale.LC_ALL, '')
    # Use better default than ascii with posix filesystems that deal in bytes
    # natively even when the C locale or no locale at all is given. Note that
    # we need an immortal string for the hack, hence the lack of a hyphen.
    sys._brz_default_fs_enc = "utf8"

    import breezy  # noqa: E402
    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    parser = argparse.ArgumentParser(prog='multi-arch-fixer')
    parser.add_argument(
        '--directory', metavar='DIRECTORY', help='directory to run in',
        type=str, default='.')
    args = parser.parse_args()
    hints = download_multiarch_hints()
    wt, subpath = WorkingTree.open_containing(args.directory)
    with wt.get_file(os.path.join(subpath, 'debian', 'control')) as f:
        source = Deb822(f)['Source']
        source_hints = hints.get(source, [])
    if not source_hints:
        print('No hints for %s' % source)
        sys.exit(0)
    apply_multiarch_hints(wt, hints)
