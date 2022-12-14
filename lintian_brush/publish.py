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

from breezy.controldir import ControlDir
from breezy.commit import NullCommitReporter, PointlessCommit
from breezy.errors import AlreadyBranchError, AlreadyControlDirError
from breezy.forge import UnsupportedForge
from breezy.workingtree import WorkingTree
from breezy.workspace import check_clean_tree, WorkspaceDirty
from debmutate.control import ControlEditor
from debmutate.vcs import source_package_vcs, unsplit_vcs_url, split_vcs_url

from . import get_committer
from .salsa import guess_repository_url
from .vcs import determine_browser_url


def update_control_for_vcs_url(source, vcs_type, vcs_url):
    source['Vcs-' + vcs_type] = vcs_url
    source['Vcs-Browser'] = determine_browser_url('git', vcs_url)


class NoVcsLocation(Exception):
    """No VCS location specified or determined."""


class ConflictingVcsAlreadySpecified(Exception):
    """Vcs is already specified."""

    def __init__(self, vcs_type, existing_vcs_url, target_vcs_url):
        self.vcs_type = vcs_type
        self.existing_vcs_url = existing_vcs_url
        self.target_vcs_url = target_vcs_url


def update_offical_vcs(wt, subpath, repo_url=None, branch=None, committer=None,
                       force=False):
    # TODO(jelmer): Allow creation of the repository as well
    check_clean_tree(wt, wt.basis_tree(), subpath)

    debcargo_path = os.path.join(subpath, 'debian/debcargo.toml')
    control_path = os.path.join(subpath, 'debian/control')

    if wt.has_filename(debcargo_path):
        from debmutate.debcargo import DebcargoControlShimEditor
        editor = DebcargoControlShimEditor.from_debian_dir(
            wt.abspath(os.path.join(subpath, 'debian')))
    elif wt.has_filename(control_path):
        control_path = wt.abspath(control_path)
        editor = ControlEditor(control_path)
    else:
        raise FileNotFoundError(control_path)
    with editor:
        try:
            vcs_type, existing_url = source_package_vcs(editor.source)
        except KeyError:
            pass
        else:
            (existing_repo_url, existing_branch,
             existing_subpath) = split_vcs_url(existing_url)
            existing = (existing_repo_url, existing_branch,
                        existing_subpath or '.')
            if (repo_url and existing != (repo_url, branch, subpath)
                    and not force):
                raise ConflictingVcsAlreadySpecified(
                    vcs_type, existing_url,
                    unsplit_vcs_url(repo_url, branch, subpath))
            logging.debug('Using existing URL %s', existing_url)
            return existing
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
            branch = 'debian/main'
        else:
            vcs_type = 'Bzr'
            branch = None
        vcs_url = unsplit_vcs_url(repo_url, branch, subpath)
        update_control_for_vcs_url(editor.source, vcs_type, vcs_url)

    if committer is None:
        committer = get_committer(wt)

    try:
        wt.commit(
            message='Set Vcs headers.',
            allow_pointless=False,
            reporter=NullCommitReporter(),
            committer=committer,
        )
    except PointlessCommit:
        if not force:
            # This can't happen
            raise

    return repo_url, branch, subpath


def create_vcs_url(repo_url, branch):
    from breezy.forge import create_project
    try:
        create_project(repo_url)
    except AlreadyControlDirError:
        logging.debug('%s already exists', repo_url)
    else:
        logging.info('Created %s', repo_url)


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
        '--no-create', help='Do not create the repository',
        action='store_true')
    parser.add_argument(
        '--force', action='store_true')
    parser.add_argument(
        '--push', help='Push branch', action='store_true')

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

    breezy.initialize()  # type: ignore
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402
    import breezy.plugins.gitlab

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        print(get_committer(wt))
        return 0

    try:
        repo_url, branch, subpath = update_offical_vcs(
            wt, subpath, repo_url=args.url,
            force=args.force)
    except WorkspaceDirty:
        logging.info("%s: Please commit pending changes first.", wt.basedir)
        return 1
    except NoVcsLocation:
        parser.print_usage()
        return 1
    except ConflictingVcsAlreadySpecified as e:
        logging.fatal(
            'Conflicting Vcs-%s already exists: %s != %s', e.vcs_type,
            e.existing_vcs_url, e.target_vcs_url)
        return 1

    if not args.no_create:
        try:
            create_vcs_url(repo_url, branch)
        except UnsupportedForge:
            logging.error(
                'Unable to find a way to create %s', repo_url)
        except AlreadyBranchError as e:
            logging.fatal('Repository already exists at %s', e.path)

    controldir = ControlDir.open(repo_url)
    try:
        branch = controldir.create_branch(name=branch)
    except AlreadyBranchError:
        branch = controldir.open_branch(name=branch)
    wt.branch.push(branch)


if __name__ == '__main__':
    sys.exit(main())
