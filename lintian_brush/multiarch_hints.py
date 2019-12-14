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

import contextlib
import os
import re
import sys

from urllib.request import urlopen, Request

from lintian_brush import (
    Fixer,
    NoChanges,
    NotDebianPackage,
    PendingChanges,
    FixerResult,
    min_certainty,
    USER_AGENT,
    SUPPORTED_CERTAINTIES,
    DEFAULT_URLLIB_TIMEOUT,
    certainty_sufficient,
    get_committer,
    get_dirty_tracker,
    check_clean_tree,
    run_lintian_fixer,
    version_string,
    )
from .control import (
    update_control,
    format_relations,
    parse_relations,
    )


MULTIARCH_HINTS_URL = 'https://dedup.debian.net/static/multiarch-hints.yaml.xz'


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
    return data['hints']


def multiarch_hints_by_binary(hints):
    ret = {}
    for entry in hints:
        ret.setdefault(entry['binary'], []).append(entry)
    return ret


def multiarch_hints_by_source(hints):
    ret = {}
    for entry in hints:
        if 'source' not in entry:
            continue
        ret.setdefault(entry['source'], []).append(entry)
    return ret


@contextlib.contextmanager
def download_multiarch_hints(url=MULTIARCH_HINTS_URL):
    """Load multi-arch hints from a URL.

    Args:
      url: URL to read from
    Returns:
      multi-arch hints file
    """
    headers = {'User-Agent': USER_AGENT}
    with urlopen(
            Request(url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT) as f:
        if url.endswith('.xz'):
            import lzma
            # It would be nicer if there was a content-type, but there isn't
            # :-(
            f = lzma.LZMAFile(f)
        yield f


def apply_hint_ma_foreign(binary, hint):
    if binary.get('Multi-Arch') != 'foreign':
        binary['Multi-Arch'] = 'foreign'
        return 'Add Multi-Arch: foreign.'


def apply_hint_ma_foreign_lib(binary, hint):
    if binary.get('Multi-Arch') == 'foreign':
        del binary['Multi-Arch']
        return 'Drop Multi-Arch: foreign.'


def apply_hint_file_conflict(binary, hint):
    if binary.get('Multi-Arch') == 'same':
        del binary['Multi-Arch']
        return 'Drop Multi-Arch: same.'


def apply_hint_dep_any(binary, hint):
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
                if r.name == dep and r.archqual != 'any':
                    r.archqual = 'any'
                    changed = True
    if not changed:
        return
    binary['Depends'] = format_relations(relations)
    return ('Add :all qualifier for %s dependency.' % dep)


def apply_hint_ma_same(binary, hint):
    if binary.get('Multi-Arch') == 'same':
        return
    binary['Multi-Arch'] = 'same'
    return 'Add Multi-Arch: same.'


def apply_hint_arch_all(binary, hint):
    if binary['Architecture'] == 'all':
        return
    binary['Architecture'] = 'all'
    return 'Make package Architecture: all.'


class MultiArchHintApplier(object):

    def __init__(self, kind, fn, certainty):
        self.kind = kind
        self.fn = fn
        self.certainty = certainty


class MultiArchFixerResult(FixerResult):

    def __init__(self, description, certainty, changes):
        super(MultiArchFixerResult, self).__init__(
            description=description, certainty=certainty)
        self.changes = changes


class MultiArchHintFixer(Fixer):

    def __init__(self, appliers, hints):
        super(MultiArchHintFixer, self).__init__(name='multiarch-hints')
        self._appliers = {applier.kind: applier for applier in appliers}
        self._hints = hints

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty=None, trust_package=False,
            allow_reformatting=False, net_access=True, opinionated=False):
        if not net_access:
            # This should never happen - perhaps if something else imported and
            # used this class?
            raise NoChanges()
        changes = []

        def update_binary(binary):
            for hint in self._hints.get(binary['Package'], []):
                kind = hint['link'].rsplit('#', 1)[1]
                applier = self._appliers[kind]
                if not certainty_sufficient(
                        applier.certainty, minimum_certainty):
                    continue
                description = applier.fn(binary, hint)
                if description:
                    changes.append(
                        (binary, hint, description, applier.certainty))

        old_cwd = os.getcwd()
        try:
            os.chdir(basedir)
            update_control(
                binary_package_cb=update_binary)
        finally:
            os.chdir(old_cwd)

        overall_certainty = min_certainty(
            [certainty for (binary, hint, description, certainty) in changes])
        overall_description = "Apply multi-arch hints.\n\n" + "\n".join(
            ["* %s: %s" % (binary['Package'], description)
             for (binary, hint, description, certainty) in changes])
        return MultiArchFixerResult(
            overall_description, certainty=overall_certainty, changes=changes)


APPLIERS = [
    MultiArchHintApplier('ma-foreign', apply_hint_ma_foreign, 'certain'),
    MultiArchHintApplier('file-conflict', apply_hint_file_conflict, 'certain'),
    MultiArchHintApplier(
        'ma-foreign-library', apply_hint_ma_foreign_lib, 'certain'),
    MultiArchHintApplier('dep-any', apply_hint_dep_any, 'certain'),
    MultiArchHintApplier('ma-same', apply_hint_ma_same, 'certain'),
    MultiArchHintApplier('arch-all', apply_hint_arch_all, 'possible'),
]


def main(argv=None):
    import argparse
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
    from breezy.trace import note  # noqa: E402

    from .config import Config

    parser = argparse.ArgumentParser(prog='multi-arch-fixer')
    parser.add_argument(
        '--directory', metavar='DIRECTORY', help='directory to run in',
        type=str, default='.')
    parser.add_argument(
        '--disable-inotify', action='store_true', default=False,
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--identity',
        help='Print user identity that would be used when committing',
        action='store_true', default=False)
    # Hide the minimum-certainty option for the moment.
    parser.add_argument(
        '--minimum-certainty',
        type=str,
        choices=SUPPORTED_CERTAINTIES,
        default=None,
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--no-update-changelog', action="store_false", default=None,
        dest="update_changelog", help="do not update the changelog")
    parser.add_argument(
        '--update-changelog', action="store_true", dest="update_changelog",
        help="force updating of the changelog", default=None)
    parser.add_argument(
        '--version', action='version', version='%(prog)s ' + version_string)
    parser.add_argument(
        '--allow-reformatting', default=None, action='store_true',
        help=argparse.SUPPRESS)

    args = parser.parse_args(argv)
    minimum_certainty = args.minimum_certainty
    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        note(get_committer(wt))
        return 0

    update_changelog = args.update_changelog
    allow_reformatting = args.allow_reformatting
    try:
        cfg = Config.from_workingtree(wt, subpath)
    except FileNotFoundError:
        pass
    else:
        if minimum_certainty is None:
            minimum_certainty = cfg.minimum_certainty()
        if allow_reformatting is None:
            allow_reformatting = cfg.allow_reformatting()
        if update_changelog is None:
            update_changelog = cfg.update_changelog()

    use_inotify = (False if args.disable_inotify else None),
    try:
        check_clean_tree(wt)
    except PendingChanges:
        note("%s: Please commit pending changes first.", wt.basedir)
        return 1

    dirty_tracker = get_dirty_tracker(wt, subpath, use_inotify)
    if dirty_tracker:
        dirty_tracker.mark_clean()

    note("Downloading multiarch hints.")
    with download_multiarch_hints() as f:
        hints = multiarch_hints_by_binary(parse_multiarch_hints(f))

    try:
        result, summary = run_lintian_fixer(
            wt, MultiArchHintFixer(APPLIERS, hints),
            update_changelog=update_changelog,
            minimum_certainty=minimum_certainty,
            dirty_tracker=dirty_tracker,
            subpath=subpath, allow_reformatting=allow_reformatting,
            net_access=True)
    except NoChanges:
        note('Nothing to do.')
    except NotDebianPackage:
        note("%s: Not a debian package.", wt.basedir)
        return 1
    else:
        for binary, description, certainty in result.changes:
            note('%s: %s' % (binary['Package'], description))


if __name__ == '__main__':
    sys.exit(main())
