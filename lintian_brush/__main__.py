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

import argparse
import atexit
import shutil
import sys
import tempfile

import distro_info

from breezy.branch import Branch
from breezy.workingtree import WorkingTree
import locale
locale.setlocale(locale.LC_ALL, '')
# Use better default than ascii with posix filesystems that deal in bytes
# natively even when the C locale or no locale at all is given. Note that
# we need an immortal string for the hack, hence the lack of a hyphen.
sys._brz_default_fs_enc = "utf8"

import breezy  # noqa: E402
from breezy.errors import (
    DependencyNotPresent,  # noqa: E402
    NotBranchError,
    )
breezy.initialize()
import breezy.git  # noqa: E402
import breezy.bzr  # noqa: E402

from breezy.trace import note, show_error  # noqa: E402

from . import (  # noqa: E402
    NotDebianPackage,
    PendingChanges,
    available_lintian_fixers,
    find_fixers_dir,
    get_committer,
    run_lintian_fixers,
    select_fixers,
    version_string,
    SUPPORTED_CERTAINTIES,
    DEFAULT_MINIMUM_CERTAINTY,
    )
from .config import Config   # noqa: E402


def main(argv=None):
    parser = argparse.ArgumentParser(prog='lintian-brush')
    parser.add_argument(
        '--no-update-changelog', action="store_false", default=None,
        dest="update_changelog", help="do not update the changelog")
    parser.add_argument(
        '--update-changelog', action="store_true", dest="update_changelog",
        help="force updating of the changelog", default=None)
    parser.add_argument(
        '--version', action='version', version='%(prog)s ' + version_string)
    parser.add_argument(
        '--list-fixers', action="store_true", help="list available fixers")
    parser.add_argument(
        '--list-tags', action="store_true",
        help="list lintian tags for which fixers are available")
    parser.add_argument(
        '--fixers-dir', type=str, help='path to fixer scripts. [%(default)s]',
        default=find_fixers_dir())
    parser.add_argument(
        '--verbose', help='be verbose', action='store_true', default=False)
    parser.add_argument(
        '--directory', metavar='DIRECTORY', help='directory to run in',
        type=str, default='.')
    parser.add_argument(
        '--diff', help='Print resulting diff afterwards.', action='store_true')
    parser.add_argument(
        '--dry-run', help=(
            'Do not make any changes to the current repository. '
            'Note: currently creates a temporary clone of the repository.'),
        action='store_true')
    parser.add_argument(
        '--modern', help=(
            'Use features/compatibility levels that are not available in '
            'stable. (makes backporting harder)'),
        action='store_true', default=False)
    parser.add_argument(
        '--identity',
        help='Print user identity that would be used when committing',
        action='store_true', default=False)
    parser.add_argument(
        '--disable-net-access',
        help='Do not probe external services.',
        action='store_true', default=False)
    # Hide the minimum-certainty option for the moment.
    parser.add_argument(
        '--minimum-certainty',
        type=str,
        choices=SUPPORTED_CERTAINTIES,
        default=None,
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--trust',
        action='store_true',
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--allow-reformatting', default=None, action='store_true',
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--disable-inotify', action='store_true', default=False,
        help=argparse.SUPPRESS)
    parser.add_argument('--compat-release', type=str, help=argparse.SUPPRESS)
    parser.add_argument(
        '--exclude', metavar='EXCLUDE', type=str, action='append',
        help='Exclude a fixer.')
    parser.add_argument(
        '--opinionated', action='store_true',
        help=argparse.SUPPRESS)
    parser.add_argument(
        'fixers', metavar='FIXER', nargs='*',
        help='specific fixer to run')
    args = parser.parse_args(argv)

    if args.list_fixers and args.list_tags:
        parser.print_usage()
        return 1

    fixers = available_lintian_fixers(args.fixers_dir)
    if args.list_fixers:
        for script in sorted([fixer.name for fixer in fixers]):
            note(script)
    elif args.list_tags:
        tags = set()
        for fixer in fixers:
            tags.update(fixer.lintian_tags)
        for tag in sorted(tags):
            note(tag)
    else:
        try:
            if args.dry_run:
                branch, subpath = Branch.open_containing(args.directory)
                td = tempfile.mkdtemp()
                atexit.register(shutil.rmtree, td)
                # TODO(jelmer): Make a slimmer copy
                to_dir = branch.controldir.sprout(
                    td, None, create_tree_if_local=True,
                    source_branch=branch,
                    stacked=branch._format.supports_stacking())
                wt = to_dir.open_workingtree()
            else:
                wt, subpath = WorkingTree.open_containing(args.directory)
        except NotBranchError:
            note('No version control directory found (e.g. a .git directory).')
            return 1
        except DependencyNotPresent as e:
            note('Unable to open tree at %s: missing package %s',
                 args.directory, e.library)
            return 1
        if args.identity:
            print(get_committer(wt))
            return 0
        since_revid = wt.last_revision()
        if args.fixers:
            try:
                fixers = select_fixers(fixers, args.fixers, args.exclude)
            except KeyError as e:
                show_error('Unknown fixer specified: %s', e.args[0])
                return 1
        debian_info = distro_info.DebianDistroInfo()
        if args.modern:
            if args.compat_release:
                show_error('--compat-release and --modern are incompatible.')
                return 1
            compat_release = debian_info.devel()
        else:
            compat_release = args.compat_release
        minimum_certainty = args.minimum_certainty
        allow_reformatting = args.allow_reformatting
        update_changelog = args.update_changelog
        try:
            cfg = Config.from_workingtree(wt, subpath)
        except FileNotFoundError:
            pass
        else:
            if minimum_certainty is None:
                minimum_certainty = cfg.minimum_certainty()
            if compat_release is None:
                compat_release = cfg.compat_release()
                if compat_release:
                    compat_release = debian_info.codename(
                        compat_release, default=compat_release)
            if allow_reformatting is None:
                allow_reformatting = cfg.allow_reformatting()
            if update_changelog is None:
                update_changelog = cfg.update_changelog()
        if minimum_certainty is None:
            minimum_certainty = DEFAULT_MINIMUM_CERTAINTY
        if compat_release is None:
            compat_release = debian_info.stable()
        if allow_reformatting is None:
            allow_reformatting = False
        with wt.lock_write():
            try:
                overall_result = run_lintian_fixers(
                    wt, fixers,
                    update_changelog=update_changelog,
                    compat_release=compat_release,
                    verbose=args.verbose,
                    minimum_certainty=args.minimum_certainty,
                    trust_package=args.trust,
                    allow_reformatting=allow_reformatting,
                    use_inotify=(False if args.disable_inotify else None),
                    subpath=subpath,
                    net_access=not args.disable_net_access,
                    opinionated=args.opinionated)
            except NotDebianPackage:
                note("%s: Not a debian package.", wt.basedir)
                return 1
            except PendingChanges:
                note("%s: Please commit pending changes first.", wt.basedir)
                if args.verbose:
                    from breezy.status import show_tree_status
                    show_tree_status(wt)
                return 1
        if overall_result.success:
            all_tags = set()
            for result, summary in overall_result.success:
                all_tags.update(result.fixed_lintian_tags)
            if all_tags:
                note("Lintian tags fixed: %r" % all_tags)
            else:
                note("Some changes were made, "
                     "but there are no affected lintian tags.")
        else:
            note("No changes made.")
        if overall_result.failed_fixers and not args.verbose:
            note("Some fixer scripts failed to run: %r. "
                 "Run with --verbose for details.",
                 set(overall_result.failed_fixers.keys()))
        if overall_result.formatting_unpreservable and not args.verbose:
            note('Some fixer scripts were unable to preserve formatting: %r. '
                 'Run with --allow-reformatting to reformat %r.',
                 set(overall_result.formatting_unpreservable),
                 set(overall_result.formatting_unpreservable.values()))
        if args.diff:
            from breezy.diff import show_diff_trees
            show_diff_trees(
                wt.branch.repository.revision_tree(since_revid),
                wt, sys.stdout.buffer)


if __name__ == '__main__':
    sys.exit(main())
