#!/usr/bin/python3

# Copyright (C) 2021 Jelmer Vernooij
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

"""Apply transitions."""

import json
import logging
import os
import re

from breezy.workingtree import WorkingTree
from breezy.workspace import (
    check_clean_tree,
    WorkspaceDirty,
    )

from debmutate.ben import parse_ben, SUPPORTED_KEYS
from debmutate.control import ControlEditor
from debmutate.deb822 import ChangeConflict
from debmutate.reformatting import (
    FormattingUnpreservable,
    GeneratedFile,
    )

from . import (
    control_files_in_root,
    get_committer,
    NotDebianPackage,
    version_string,
    )
from .changelog import add_changelog_entry
from .config import Config


class TransitionResult(object):

    def __init__(self, ben, bugno=None):
        self.ben = ben
        self.bugno = bugno


_changelog_policy_noted = None


def _note_changelog_policy(policy, msg):
    global _changelog_policy_noted
    if not _changelog_policy_noted:
        if policy:
            extra = "Specify --no-update-changelog to override."
        else:
            extra = "Specify --update-changelog to override."
        logging.info("%s %s", msg, extra)
    _changelog_policy_noted = True


def control_matches(control, ors):
    for field, expr in ors:
        if not field.startswith('.'):
            raise ValueError('unsupported field %r' % field)
        for paragraph in control.paragraphs:
            try:
                value = paragraph[field[1:]]
            except KeyError:
                continue
            if expr.findall(value):
                return True
    return False


class PackageNotAffected(Exception):

    def __init__(self, source):
        self.source = source


class PackageAlreadyGood(Exception):

    def __init__(self, source):
        self.source = source


class PackageNotBad(Exception):

    def __init__(self, source):
        self.source = source


def ben_find_bugno(ben):
    bugs = re.findall('#([0-9]+)', ben.get('notes', ''))

    if bugs:
        return int(bugs[0])
    else:
        return None


def _apply_transition(control, ben):
    for key in ben:
        if key not in SUPPORTED_KEYS:
            raise ValueError('unsupported key in ben file: %r' % key)
    if ben.get('is_affected'):
        if not control_matches(control, ben['is_affected']):
            raise PackageNotAffected(control.source['Source'])
    if ben.get('is_good'):
        if control_matches(control, ben['is_good']):
            raise PackageAlreadyGood(control.source['Source'])
    if ben.get('is_bad'):
        if not control_matches(control, ben['is_bad']):
            raise PackageNotBad(control.source['Source'])
    for field, expr in ben['is_bad']:
        if not field.startswith('.'):
            raise ValueError('unsupported field %r' % field)
        for paragraph in control.paragraphs:
            try:
                value = paragraph[field[1:]]
            except KeyError:
                continue
            for goodfield, goodexpr in ben['is_good']:
                if goodfield == field:
                    replacement = goodexpr.pattern
                    break
            else:
                raise ValueError(
                    'unable to find replacement value for %s=%s' % field, expr)
            paragraph[field[1:]] = expr.sub(replacement, value)

    bugno = ben_find_bugno(ben)

    return TransitionResult(ben, bugno=bugno)


def report_fatal(code: str, description: str) -> None:
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'description': description}, f)
    logging.fatal('%s', description)


def report_okay(code: str, description: str) -> None:
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'description': description}, f)
    logging.info('%s', description)


def apply_transition(
        wt, debian_path, ben, update_changelog, allow_reformatting):
    control_path = os.path.join(debian_path, "control")
    try:
        with ControlEditor(
                wt.abspath(control_path),
                allow_reformatting=allow_reformatting) as editor:
            return _apply_transition(editor, ben)
    except FileNotFoundError as exc:
        raise NotDebianPackage(wt, debian_path) from exc


def main():  # noqa: C901
    import argparse
    import breezy  # noqa: E402

    breezy.initialize()  # type: ignore
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    parser = argparse.ArgumentParser(prog="deb-transition-apply")
    parser.add_argument(
        "--directory",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )
    parser.add_argument(
        "--no-update-changelog",
        action="store_false",
        default=None,
        dest="update_changelog",
        help="do not update the changelog",
    )
    parser.add_argument(
        "--update-changelog",
        action="store_true",
        dest="update_changelog",
        help="force updating of the changelog",
        default=None,
    )
    parser.add_argument(
        "--allow-reformatting",
        default=None,
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument(
        "--identity",
        help="Print user identity that would be used when committing",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--debug", help="Describe all considered changes.", action="store_true"
    )
    parser.add_argument(
        "benfile", help="Benfile to read transition from.", type=str
    )

    args = parser.parse_args()

    with open(args.benfile, 'r') as f:
        ben = parse_ben(f)

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        logging.info('%s', get_committer(wt))
        return 0

    try:
        check_clean_tree(wt, wt.basis_tree(), subpath)
    except WorkspaceDirty:
        logging.info("%s: Please commit pending changes first.", wt.basedir)
        return 1

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format='%(message)s')

    update_changelog = args.update_changelog
    allow_reformatting = args.allow_reformatting

    try:
        cfg = Config.from_workingtree(wt, subpath)
    except FileNotFoundError:
        pass
    else:
        if update_changelog is None:
            update_changelog = cfg.update_changelog()
        if allow_reformatting is None:
            allow_reformatting = cfg.allow_reformatting()

    if allow_reformatting is None:
        allow_reformatting = False

    if control_files_in_root(wt, subpath):
        debian_path = subpath
    else:
        debian_path = os.path.join(subpath, 'debian')

    try:
        result = apply_transition(
            wt, debian_path, ben, update_changelog=args.update_changelog,
            allow_reformatting=allow_reformatting
        )
    except PackageNotAffected:
        report_okay(
            "nothing-to-do",
            "Package not affected by transition")
        return 0
    except PackageAlreadyGood:
        report_okay(
            "nothing-to-do",
            "Transition already applied to package")
        return 0
    except PackageNotBad:
        report_okay(
            "nothing-to-do",
            "Package not bad")
        return 0
    except FormattingUnpreservable as e:
        report_fatal(
            "formatting-unpreservable",
            "unable to preserve formatting while editing %s" % e.path,
        )
        return 1
    except GeneratedFile as e:
        report_fatal(
            "generated-file", "unable to edit generated file: %r" % e
        )
        return 1
    except NotDebianPackage:
        report_fatal('not-debian-package', 'Not a Debian package.')
        return 1
    except ChangeConflict as e:
        report_fatal(
            'change-conflict',
            'Generated file changes conflict: %s' % e)
        return 1

    if not result:
        report_okay("nothing-to-do", "no changes from transition")
        return 0

    changelog_path = os.path.join(debian_path, "changelog")

    if update_changelog is None:
        from .detect_gbp_dch import guess_update_changelog
        from debian.changelog import Changelog

        with wt.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)

        dch_guess = guess_update_changelog(wt, debian_path, cl)
        if dch_guess:
            update_changelog = dch_guess[0]
            _note_changelog_policy(update_changelog, dch_guess[1])
        else:
            # Assume we should update changelog
            update_changelog = True

    if update_changelog:
        summary = 'Apply transition %s.' % ben['title']
        if result.bugno:
            summary += ' Closes: #%d' % result.bugno
        add_changelog_entry(wt, changelog_path, [summary])

    if os.environ.get("SVP_API") == "1":
        with open(os.environ["SVP_RESULT"], "w") as f:
            json.dump({
                "description": "Apply transition.",
                "value": result.value(),
                "context": ben
            }, f)

    logging.info("Applied transition %s", ben['title'])
    return 0


if __name__ == "__main__":
    import sys

    sys.exit(main())
