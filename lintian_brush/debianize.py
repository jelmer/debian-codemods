#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij
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

"""Debianize a package."""

import os
import sys
from urllib.parse import urlparse
import warnings


from debian.changelog import Changelog, Version, get_maintainer, format_date
from debmutate.control import ensure_some_version
from debian.deb822 import Deb822

from breezy import osutils
from breezy.errors import AlreadyBranchError
from breezy.commit import NullCommitReporter
from breezy.trace import note, warning  # noqa: E402

from upstream_ontologist.guess import (
    get_upstream_info,
    )
from upstream_ontologist.debian import (
    upstream_name_to_debian_source_name as source_name_from_upstream_name,
    upstream_version_to_debian_upstream_version as debian_upstream_version,
    valid_debian_package_name,
    )

from . import (
    available_lintian_fixers,
    version_string,
    check_clean_tree,
    PendingChanges,
    get_dirty_tracker,
    run_lintian_fixers,
    get_committer,
    reset_tree,
    )
from .debhelper import (
    maximum_debhelper_compat_version,
    write_rules_template as write_debhelper_rules_template,
    )
from .standards_version import iter_standards_versions


def write_control_template(path, source, binaries):
    """Write a control file template.

    Args:
      path: Path to write to
      source: Source stanza
      binaries: Binary stanzas
    """
    with open(path, 'wb') as f:
        source.dump(f)
        for binary in binaries:
            f.write(b'\n')
            binary.dump(f)


def write_changelog_template(path, source_name, version, wnpp_bugs=None):
    if wnpp_bugs:
        closes = ' Closes: ' + ', '.join(
            [('#%d' % (bug, )) for bug in wnpp_bugs])
    else:
        closes = ''
    cl = Changelog()
    cl.new_block(
        package=source_name,
        version=version,
        distributions='UNRELEASED',
        urgency='low',
        changes=['', '  * Initial release.' + closes, ''],
        author='%s <%s>' % get_maintainer(),
        date=format_date())
    with open(path, 'w') as f:
        f.write(cl.__str__().strip('\n') + '\n')


async def find_archived_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        warnings.warn('asyncpg not available, unable to find wnpp bugs.')
        return []
    conn = await connect_udd_mirror()
    return [row[0] for row in await conn.fetch("""\
select id from archived_bugs where package = 'wnpp' and
title like 'ITP: ' || $1 || ' -- %' OR
title like 'RFP: ' || $1 || ' -- %'
""", source_name)]


async def find_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        warnings.warn('asyncpg not available, unable to find wnpp bugs.')
        return []
    conn = await connect_udd_mirror()
    return [row[0] for row in await conn.fetch("""\
select id from wnpp where source = $1 and type in ('ITP', 'RFP')
""", source_name)]


MINIMUM_CERTAINTY = 'possible'  # For now..


class DebianDirectoryExists(Exception):
    """A Debian Directory already exists."""

    def __init__(self, path):
        self.path = path


def go_import_path_from_repo(repo_url):
    parsed_url = urlparse(repo_url)
    p = parsed_url.hostname + parsed_url.path
    if p.endswith('.git'):
        p = p[:-4]
    return p


def debianize(
        wt, subpath, use_inotify=None, diligence=0, trust=False, check=False,
        net_access=True, force_subprocess=False, compat_release=None,
        minimum_certainty=MINIMUM_CERTAINTY,
        consult_external_directory=True,
        verbose=False):
    dirty_tracker = get_dirty_tracker(wt, subpath, use_inotify)
    if dirty_tracker:
        dirty_tracker.mark_clean()

    if os.path.exists('debian') and list(os.listdir('debian')):
        raise DebianDirectoryExists(wt.abspath(subpath))

    # TODO(jelmer): Find revision with latest release rather than simply
    # last revision?
    try:
        wt.controldir.create_branch('upstream').generate_revision_history(
            wt.last_revision())
    except AlreadyBranchError:
        note('Upstream branch already exists; not creating.')
    else:
        note('Created upstream branch.')

    buildsystem, unused_requirements, metadata = (
        get_upstream_info(
            wt.abspath(subpath), trust_package=trust,
            net_access=net_access,
            consult_external_directory=consult_external_directory,
            check=check))

    try:
        upstream_name = metadata['Name']
    except KeyError:
        note('%s: Unable to determine upstream package name.',
             wt.abspath(subpath))
        if not trust:
            note('Run with --trust if you are okay running code '
                 'from the package?')
        return 1

    source_name = source_name_from_upstream_name(upstream_name)

    with wt.lock_write():
        try:
            from breezy.plugins.debian.upstream.branch import (
                upstream_branch_version,
                upstream_version_add_revision,
                )
        except ModuleNotFoundError:
            note('Install breezy-debian for upstream version guessing.')
        else:
            upstream_version = upstream_branch_version(
                wt.branch, wt.last_revision(), upstream_name)
            if upstream_version is None and 'X-Version' in metadata:
                # They haven't done any releases yet. Assume we're ahead of
                # the next announced release?
                next_upstream_version = debian_upstream_version(
                    metadata['X-Version'])
                upstream_version = upstream_version_add_revision(
                    wt.branch, next_upstream_version, wt.last_revision(),
                    '~')
        if upstream_version is None:
            note('Unable to determine upstream version, using 0.')

        version = Version(upstream_version + '-1')
        source = Deb822()
        # TODO(jelmer): This is a reasonable guess, but won't always be
        # okay.
        source['Rules-Requires-Root'] = 'no'
        source['Standards-Version'] = '.'.join(
            map(str, next(iter_standards_versions())[0]))

        binaries = []
        source['Build-Depends'] = (
            'debhelper-compat (= %d)' % maximum_debhelper_compat_version(
                compat_release))
        dh_addons = []
        initial_files = []
        dh_buildsystem = None

        if buildsystem and buildsystem.name == 'setup.py':
            dh_buildsystem = 'pybuild'
            dh_addons.append('python3')
            source_name = 'python-%s' % upstream_name
            binaries.append(
                Deb822({'Package': 'python3-%s' % source_name,
                        'Architecture': 'all'}))
        elif buildsystem and buildsystem.name == 'npm':
            dh_addons.append('nodejs')
            source_name = 'node-%s' % upstream_name
            binaries.append(
                Deb822({'Package': 'node-%s' % upstream_name,
                        'Architecture': 'all'}))
            if os.path.exists('test/node.js'):
                source['Testsuite'] = 'autopkgtest-pkg-nodejs'
                os.makedirs('debian/tests/pkg-js', exist_ok=True)
                initial_files.append('debian/tests/pkg-js/test')
                with open('debian/tests/pkg-js/test', 'w') as f:
                    f.write('mocha test/node.js')
                source['Build-Depends'] = ensure_some_version(
                    source['Build-Depends'], 'mocha <!nocheck>')
        elif buildsystem and buildsystem.name == 'dist-zilla':
            source_name = 'lib%s-perl' % upstream_name
            dh_addons.append('dist-zilla')
            binaries.append(
                Deb822({'Package': 'lib%s-perl' % upstream_name,
                        'Architecture': 'all'}))
        elif buildsystem and buildsystem.name == 'cargo':
            source_name = 'rust-%s' % upstream_name
            source['Build-Depends'] = ensure_some_version(
                source['Build-Depends'], 'dh-cargo')
            dh_buildsystem = 'cargo'
            binaries.append(
                Deb822({'Package': 'rust-%s' % source_name,
                        'Architecture': 'any'}))
        elif buildsystem and buildsystem.name == 'golang':
            source['XS-Go-Import-Path'] = go_import_path_from_repo(
                metadata['Repository'])
            if 'Repository-Browse' in metadata:
                source['Homepage'] = metadata['Repository-Browse']
            source['Section'] = 'devel'
            parsed_url = urlparse(metadata['Repository-Browse'])
            hostname = parsed_url.hostname
            if hostname == 'github.com':
                hostname = 'github'
            godebname = hostname + parsed_url.path.replace('/', '-')
            source_name = 'golang-%s' % godebname
            source['Testsuite'] = 'autopkgtest-pkg-go'
            dh_addons.append('golang')
            dh_buildsystem = 'golang'
            # TODO(jelmer): Add --builddirectory=_build to dh arguments
            binaries.append(
                Deb822({'Package': 'golang-%s-dev' % godebname,
                        'Architecture': 'all',
                        'Multi-Arch': 'foreign'}))
        else:
            source_name = upstream_name
            for binary_name, arch in [(source_name, 'any')]:
                binaries.append(
                    Deb822({'Package': binary_name, 'Architecture': arch}))

        for dh_addon in dh_addons:
            source['Build-Depends'] = ensure_some_version(
                source['Build-Depends'],
                'dh-sequence-%s' % dh_addon)

        if not valid_debian_package_name(source_name):
            note('Unable to sanitize source package name: %s',
                 source_name)
            return 1

        source['Source'] = source_name

        if net_access:
            import asyncio
            loop = asyncio.get_event_loop()
            wnpp_bugs = loop.run_until_complete(find_wnpp_bugs(source_name))
            if not wnpp_bugs:
                wnpp_bugs = loop.run_until_complete(
                    find_archived_wnpp_bugs(source_name))
                if wnpp_bugs:
                    warning('Found archived ITP/RFP bugs for %s: %r',
                            source_name, wnpp_bugs)
                else:
                    warning('No relevant WNPP bugs found for %s', source_name)
            else:
                note('Found WNPP bugs for %s: %r', source_name, wnpp_bugs)
        else:
            wnpp_bugs = None

        try:
            debian_path = osutils.pathjoin(subpath, 'debian')
            if not wt.has_filename(debian_path):
                wt.mkdir(debian_path)
            write_debhelper_rules_template(
                wt.abspath(os.path.join(debian_path, 'rules')),
                buildsystem=dh_buildsystem)
            initial_files.append('debian/rules')
            write_control_template(
                wt.abspath(os.path.join(debian_path, 'control')),
                source, binaries)
            initial_files.append('debian/control')
            write_changelog_template(
                wt.abspath(os.path.join(debian_path, 'changelog')),
                source['Source'], version, wnpp_bugs)
            initial_files.append('debian/changelog')

            wt_paths = [osutils.pathjoin(subpath, p) for p in initial_files]
            wt.add(wt_paths)

            wt.commit(
                'Create debian/ directory', allow_pointless=False,
                committer=get_committer(wt),
                specific_files=wt_paths,
                reporter=NullCommitReporter())
        except BaseException:
            reset_tree(
                wt, wt.basis_tree(), subpath, dirty_tracker=dirty_tracker)
            raise

        fixers = available_lintian_fixers(
            force_subprocess=force_subprocess)

        run_lintian_fixers(
            wt, fixers,
            update_changelog=False,
            compat_release=compat_release,
            verbose=verbose,
            minimum_certainty=minimum_certainty,
            trust_package=trust,
            allow_reformatting=True,
            use_inotify=use_inotify,
            subpath=subpath,
            net_access=net_access,
            opinionated=True,
            diligence=diligence)


def main(argv=None):
    import argparse
    from breezy.workingtree import WorkingTree

    import breezy  # noqa: E402
    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    parser = argparse.ArgumentParser(prog='debianize')
    parser.add_argument(
        '--directory', metavar='DIRECTORY', help='directory to run in',
        type=str, default='.')
    parser.add_argument(
        '--disable-inotify', action='store_true', default=False,
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--version', action='version', version='%(prog)s ' + version_string)
    parser.add_argument('--compat-release', type=str, help=argparse.SUPPRESS)
    parser.add_argument(
        '--verbose', help='be verbose', action='store_true', default=False)
    parser.add_argument(
        '--disable-net-access',
        help='Do not probe external services.',
        action='store_true', default=False)
    parser.add_argument(
        '--diligent', action='count', default=0, dest='diligence',
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--trust',
        action='store_true',
        help='Whether to allow running code from the package.')
    parser.add_argument(
        '--consult-external-directory',
        action='store_true',
        help='Pull in external (not maintained by upstream) directory data')
    parser.add_argument(
        '--check', action='store_true',
        help='Check guessed metadata against external sources.')
    parser.add_argument(
        '--force-subprocess', action='store_true',
        help=argparse.SUPPRESS)

    args = parser.parse_args(argv)

    compat_release = args.compat_release
    if compat_release is None:
        import distro_info
        debian_info = distro_info.DebianDistroInfo()
        compat_release = debian_info.stable()

    wt, subpath = WorkingTree.open_containing(args.directory)

    use_inotify = (False if args.disable_inotify else None),
    with wt.lock_write():
        try:
            check_clean_tree(wt, wt.basis_tree(), subpath)
        except PendingChanges:
            note("%s: Please commit pending changes first.", wt.basedir)
            return 1

        try:
            debianize(
                wt, subpath, use_inotify=use_inotify,
                diligence=args.diligence,
                trust=args.trust,
                check=args.check,
                net_access=not args.disable_net_access,
                force_subprocess=args.force_subprocess,
                compat_release=compat_release,
                consult_external_directory=args.consult_external_directory,
                verbose=args.verbose)
        except DebianDirectoryExists as e:
            note('%s: A debian directory already exists. '
                 'Run lintian-brush instead?', e.path)
            return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
