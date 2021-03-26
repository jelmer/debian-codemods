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

"""Publish a package."""

from email.utils import parseaddr
import logging
import os
import sys

from breezy.commit import NullCommitReporter
from breezy.errors import AlreadyBranchError
from breezy.workingtree import WorkingTree
from debmutate.control import ControlEditor
from debmutate.vcs import source_package_vcs

from . import get_committer, check_clean_tree, PendingChanges
from .salsa import guess_repository_url
from .vcs import determine_browser_url


def update_control_for_vcs_url(source, vcs_type, repo_url):
    source['Vcs-' + vcs_type] = repo_url
    source['Vcs-Browser'] = determine_browser_url('git', repo_url)


class NoVcsLocation(Exception):
    """No VCS location specified or determined."""


class VcsAlreadySpecified(Exception):
    """Vcs is already specified."""


def update_offical_vcs(wt, subpath, repo_url=None, committer=None):
    # TODO(jelmer): Allow creation of the repository as well
    check_clean_tree(wt, wt.basis_tree(), subpath)

    if wt.has_filename(os.path.join(subpath, 'debian/debcargo.toml')):
        from debmutate.debcargo import DebcargoControlShimEditor, DebcargoEditor
        editor = DebcargoControlShimEditor.from_debian_dir(wt.abspath(os.path.join(subpath, 'debian')))
    else:
        control_path = wt.abspath(os.path.join(subpath, 'debian/control'))
        editor = ControlEditor(control_path)
    with editor:
        try:
            vcs_type, url = source_package_vcs(editor.source)
        except KeyError:
            pass
        else:
            raise VcsAlreadySpecified(vcs_type, url)
        maintainer_email = parseaddr(editor.source['Maintainer'])[1]
        source = editor.source['Source']
        if repo_url is None:
            repo_url = guess_repository_url(source, maintainer_email)
        if repo_url is None:
            raise NoVcsLocation()
        logging.info('Using repository URL: %s', repo_url)
        # TODO(jelmer): Detect vcs type in a better way
        if hasattr(wt.branch.repository, '_git'):
            vcs_type = 'Git'
        else:
            vcs_type = 'Bzr'
        update_control_for_vcs_url(editor.source, vcs_type, repo_url)

    if committer is None:
        committer = get_committer(wt)

    wt.commit(
        message='Set Vcs headers.',
        allow_pointless=False,
        reporter=NullCommitReporter(),
        committer=committer,
    )

    return repo_url


def main():
    import argparse
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--identity",
        help="Print user identity that would be used when committing",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--debug", help="Describe all considerd changes.", action="store_true"
    )

    parser.add_argument(
        "--directory",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )
    parser.add_argument(
        'url',
        type=str,
        help='URL to publish to.',
        nargs='?')

    args = parser.parse_args()

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO)

    import breezy  # noqa: E402

    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        print(get_committer(wt))
        return 0

    try:
        update_offical_vcs(wt, subpath, repo_url=args.url)
    except PendingChanges:
        logging.info("%s: Please commit pending changes first.", wt.basedir)
        return 1
    except NoVcsLocation:
        parser.print_usage()
        return 1
    except VcsAlreadySpecified as e:
        logging.fatal(
            'Package already in %s at %s', e.args[0], e.args[1])
        return 1
    except AlreadyBranchError as e:
        logging.fatal('Repository already exists at %s', e.path)
        return 1


if __name__ == '__main__':
    sys.exit(main())
