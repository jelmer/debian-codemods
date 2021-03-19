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

import contextlib
import logging
import os
import shutil
import sys
from typing import Optional
from urllib.parse import urlparse
import warnings


from debian.changelog import Changelog, Version, get_maintainer, format_date
from debmutate.control import ensure_some_version, ensure_exact_version, ensure_relation, ControlEditor
from debian.deb822 import PkgRelation

from breezy import osutils
from breezy.errors import AlreadyBranchError
from breezy.commit import NullCommitReporter

from ognibuild import DetailedFailure, UnidentifiedError
from ognibuild.dist import run_dist, DistCatcher, DistNoTarball
from ognibuild.session.plain import PlainSession
from ognibuild.session.schroot import SchrootSession
from ognibuild.resolver import auto_resolver
from ognibuild.buildlog import InstallFixer

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
from .publish import update_offical_vcs, NoVcsLocation
from .standards_version import latest_standards_version


class UpstreamNameUnknown(Exception):
    """Upstream name unknown."""

    def __init__(self, path):
        self.path = path


class SourcePackageNameInvalid(Exception):
    """Source package name is invalid."""

    def __init__(self, source):
        self.source = source


class DistCreationFailed(Exception):
    """Dist tarball creation failed."""

    def __init__(self, msg, inner=None):
        self.msg = msg
        self.inner = inner


def write_changelog_template(path, source_name, version, wnpp_bugs=None):
    if wnpp_bugs:
        closes = " Closes: " + ", ".join([("#%d" % (bug,)) for bug in wnpp_bugs])
    else:
        closes = ""
    cl = Changelog()
    cl.new_block(
        package=source_name,
        version=version,
        distributions="UNRELEASED",
        urgency="low",
        changes=["", "  * Initial release." + closes, ""],
        author="%s <%s>" % get_maintainer(),
        date=format_date(),
    )
    with open(path, "w") as f:
        f.write(cl.__str__().strip("\n") + "\n")


async def find_archived_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        warnings.warn("asyncpg not available, unable to find wnpp bugs.")
        return []
    conn = await connect_udd_mirror()
    return [
        row[0]
        for row in await conn.fetch(
            """\
select id from archived_bugs where package = 'wnpp' and
title like 'ITP: ' || $1 || ' -- %' OR
title like 'RFP: ' || $1 || ' -- %'
""",
            source_name,
        )
    ]


async def find_wnpp_bugs(source_name):
    try:
        from .udd import connect_udd_mirror
    except ModuleNotFoundError:
        warnings.warn("asyncpg not available, unable to find wnpp bugs.")
        return []
    conn = await connect_udd_mirror()
    return [
        row[0]
        for row in await conn.fetch(
            """\
select id from wnpp where source = $1 and type in ('ITP', 'RFP')
""",
            source_name,
        )
    ]


MINIMUM_CERTAINTY = "possible"  # For now..


class DebianDirectoryExists(Exception):
    """A Debian Directory already exists."""

    def __init__(self, path):
        self.path = path


def go_import_path_from_repo(repo_url):
    parsed_url = urlparse(repo_url)
    p = parsed_url.hostname + parsed_url.path
    if p.endswith(".git"):
        p = p[:-4]
    return p


def enable_dh_addon(source, addon):
    source["Build-Depends"] = ensure_some_version(
        source["Build-Depends"], "dh-sequence-%s" % addon
    )


def setup_debhelper(wt, debian_path, source, compat_release, addons=None, env=None, buildsystem=None):
    source["Build-Depends"] = ensure_exact_version(
            source.get("Build-Depends", ""),
            "debhelper-compat",
            str(maximum_debhelper_compat_version(compat_release)))
    for addon in addons or []:
        enable_dh_addon(source, addon)
    write_debhelper_rules_template(
        wt.abspath(os.path.join(debian_path, "rules")),
        buildsystem=buildsystem,
        env=(env or {}),
    )


def import_upstream_version_from_dist(
        wt, subpath, buildsystem, source_name, upstream_version,
<<<<<<< HEAD
        session, resolver, fixers, create_dist=None):
=======
        session, resolver, fixers):
    def create_dist(tree, package, version, target_dir):
        # TODO(jelmer): set include_controldir=True to make
        # setuptools_scm happy?
        external_dir, internal_dir = session.setup_from_vcs(
            wt, subpath)
        with DistCatcher(external_dir) as dc:
            session.chdir(internal_dir)
            try:
                run_dist(session, [buildsystem], resolver, fixers, quiet=True)
            except NotImplementedError:
                return None
            except DetailedFailure as e:
                raise DistCreationFailed(str(e.error), e.error)
            except UnidentifiedError as e:
                raise DistCreationFailed(''.join([line + '\n' for line in e.lines]))

        try:
            for path in dc.files:
                shutil.copy(path, target_dir)
                return os.path.join(target_dir, os.path.basename(path))
        finally:
            for path in dc.files:
                if os.path.isdir(path):
                    shutil.rmtree(path)
                else:
                    os.unlink(path)

        raise DistNoTarball()

>>>>>>> 9a444d6d (Fix error handling.)
    from breezy.plugins.debian import default_orig_dir
    from breezy.plugins.debian.util import debuild_config
    from breezy.plugins.debian.merge_upstream import get_tarballs, do_import
    from breezy.plugins.debian.upstream.branch import UpstreamBranchSource
    from breezy.plugins.debian.upstream.pristinetar import get_pristine_tar_source
    import tempfile

    if create_dist is None:
        def create_dist(tree, package, version, target_dir):
            # TODO(jelmer): set include_controldir=True to make
            # setuptools_scm happy?
            external_dir, internal_dir = session.setup_from_vcs(
                wt, subpath)
            with DistCatcher(external_dir) as dc:
                session.chdir(internal_dir)
                try:
                    run_dist(session, [buildsystem], resolver, fixers, quiet=True)
                except NotImplementedError:
                    return None
                except DetailedFailure as e:
                    raise DistCreationFailed(str(e), e.error)
                except UnidentifiedError as e:
                    raise DistCreationFailed(str(e))

            try:
                for path in dc.files:
                    shutil.copy(path, target_dir)
                    return os.path.join(target_dir, os.path.basename(path))
            finally:
                for path in dc.files:
                    if os.path.isdir(path):
                        shutil.rmtree(path)
                    else:
                        os.unlink(path)

            raise DistNoTarball()

    config = debuild_config(wt, subpath)
    upstream_source = UpstreamBranchSource.from_branch(
        wt.branch, config=config, local_dir=wt.controldir,
        create_dist=create_dist, snapshot=False)
    pristine_tar_source = get_pristine_tar_source(wt, wt.branch)
    tag_names = {}
    if pristine_tar_source.has_version(source_name, upstream_version):
        logging.warning(
            'Upstream version %s/%s already imported.',
            source_name, upstream_version)
        pristine_revids = pristine_tar_source\
            .version_as_revisions(source_name, upstream_version)
    else:
        with tempfile.TemporaryDirectory() as target_dir:
            locations = upstream_source.fetch_tarballs(
                source_name, upstream_version, target_dir, components=[None])
            orig_dir = config.orig_dir or default_orig_dir
            tarball_filenames = get_tarballs(
                orig_dir, wt, source_name, upstream_version, locations)
            upstream_revisions = upstream_source\
                .version_as_revisions(source_name, upstream_version)
            files_excluded = None
            imported_revids = do_import(
                wt, subpath, tarball_filenames, source_name, upstream_version,
                current_version=None, upstream_branch=wt.branch,
                upstream_revisions=upstream_revisions,
                merge_type=None, files_excluded=files_excluded)
            pristine_revids = {}
            for (component, tag_name, revid, pristine_tar_imported) in imported_revids:
                pristine_revids[component] = revid
                tag_names[component] = tag_name

    upstream_branch_name = "upstream"
    try:
        wt.controldir.create_branch(upstream_branch_name).generate_revision_history(
            pristine_revids[None]
        )
    except AlreadyBranchError:
        logging.info("Upstream branch already exists; not creating.")
    else:
        logging.info("Created upstream branch.")

    return pristine_revids, tag_names, upstream_branch_name


class ResetOnFailure(object):

    def __init__(self, wt, subpath=None, dirty_tracker=None):
        self.wt = wt
        self.subpath = subpath
        self.dirty_tracker = dirty_tracker

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type:
            reset_tree(
                self.wt, self.wt.basis_tree(), self.subpath,
                dirty_tracker=self.dirty_tracker)
        return False


def process_setup_py(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        addons=["python3"],
        buildsystem="pybuild")
    source["Testsuite"] = "autopkgtest-pkg-python"
    source["Build-Depends"] = ensure_some_version(
        source["Build-Depends"], "python3-all")
    upstream_name = metadata['Name']
    if upstream_name.startswith('python-'):
        upstream_name = upstream_name[len('python-'):]
    source['Source'] = "python-%s" % upstream_name.lower().replace('_', '-')
    control.add_binary({
            "Package": "python3-%s" % upstream_name.lower().replace('_', '-'),
            "Depends": "${python3:Depends}",
            "Architecture": "all",
        }
    )
    return control


def process_npm(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release, addons=["nodejs"])
    upstream_name = metadata['Name']
    source['Source'] = "node-%s" % upstream_name.lower()
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    control.add_binary(
        {"Package": "node-%s" % upstream_name, "Architecture": "all"})
    if wt.has_filename(os.path.join(subpath, "test/node.js")):
        source["Testsuite"] = "autopkgtest-pkg-nodejs"
        os.makedirs(
            os.path.join(debian_path, "debian/tests"), exist_ok=True)
        with open(os.path.join(debian_path, "tests/pkg-js/test"), "w") as f:
            f.write("mocha test/node.js")
        source["Build-Depends"] = ensure_some_version(
            source["Build-Depends"], "mocha <!nocheck>"
        )
    return control


def process_dist_zilla(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source['Source'] = "lib%s-perl" % upstream_name.replace('::', '-').replace('_', '').lower()
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        addons=["dist-zilla"])
    control.add_binary(
        {"Package": source['Source'],
         "Architecture": "all"
         })
    return control


def process_golang(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    source["XS-Go-Import-Path"] = go_import_path_from_repo(
        metadata["Repository"]
    )
    if "Repository-Browse" in metadata:
        source["Homepage"] = metadata["Repository-Browse"]
    source["Section"] = "devel"
    parsed_url = urlparse(metadata["Repository-Browse"])
    hostname = parsed_url.hostname
    if hostname == "github.com":
        hostname = "github"
    godebname = (hostname + parsed_url.path.replace("/", "-")).replace("_", "-").lower()
    source['Source'] = "golang-%s" % godebname
    source["Testsuite"] = "autopkgtest-pkg-go"
    dh_env = {}
    if os.path.isdir("examples"):
        dh_env["DH_GOLANG_EXCLUDES"] = "examples/"
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        addons=["golang"],
        buildsystem="golang",
        env=dh_env)
    # TODO(jelmer): Add --builddirectory=_build to dh arguments
    control.add_binary({
        "Package": "golang-%s-dev" % godebname,
        "Architecture": "all",
        "Multi-Arch": "foreign",
        })
    return control


def process_r(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source

    if metadata.get('Archive') == 'CRAN':
        archive = 'cran'
    elif metadata.get('Archive') == 'Bioconductor':
        archive = 'bioc'
    else:
        archive = 'other'

    source["Source"] = "r-%s-%s" % (archive, metadata['Name'])
    source["Rules-Requires-Root"] = "no"
    source["Build-Depends"] = "dh-r, r-base-dev"
    source["Standards-Version"] = latest_standards_version()
    source["Testsuite"] = "autopkgtest-pkg-r"
    setup_debhelper(
        wt, debian_path, source, compat_release=compat_release,
        buildsystem="R")
    # For now, just assume a single binary package that is architecture-dependent.
    control.add_binary({
        "Package": "r-%s-%s" % (archive, metadata['Name']),
        "Architecture": 'any',
        'Depends': '${R:Depends}, ${shlibs:Depends}, ${misc:Depends}',
        'Recommends': '${R:Recommends}',
        'Suggests': '${R:Suggests}',
        })
    return control


def process_default(es, wt, subpath, debian_path, metadata, compat_release):
    control = es.enter_context(ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source["Source"] = source_name_from_upstream_name(upstream_name)
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release)
    # For now, just assume a single binary package that is architecture-dependent.
    for binary_name, arch in [(source['Source'], "any")]:
        control.add_binary({"Package": binary_name, "Architecture": arch})
    return control


def process_cargo(es, wt, subpath, debian_path, metadata, compat_release):
    from debmutate.debcargo import DebcargoControlShimEditor
    upstream_name = metadata['Name']
    return es.enter_context(DebcargoControlShimEditor.from_debian_dir(wt.abspath(debian_path), upstream_name))


PROCESSORS = {
    "setup.py": process_setup_py,
    "npm": process_npm,
    "dist-zilla": process_dist_zilla,
    "cargo": process_cargo,
    "golang": process_golang,
    "R": process_r,
    }


class DebianizeResult(object):
    """Debianize result."""

    def __init__(self, upstream_branch_name, tag_names, upstream_version):
        self.upstream_branch_name = upstream_branch_name
        self.tag_names = tag_names
        self.upstream_version = upstream_version


def debianize(  # noqa: C901
    wt,
    subpath: str,
    use_inotify: Optional[bool] = None,
    diligence: int = 0,
    trust: bool = False,
    check: bool = False,
    net_access: bool = True,
    force_subprocess: bool = False,
    compat_release: Optional[str] = None,
    minimum_certainty: str = MINIMUM_CERTAINTY,
    consult_external_directory: bool = True,
    verbose: bool = False,
    schroot: Optional[str] = None,
    create_dist=None
):
    dirty_tracker = get_dirty_tracker(wt, subpath, use_inotify)
    if dirty_tracker:
        dirty_tracker.mark_clean()

    debian_path = osutils.pathjoin(subpath, "debian")
    if wt.has_filename(debian_path) and list(os.listdir(wt.abspath(debian_path))):
        raise DebianDirectoryExists(wt.abspath(subpath))

    metadata = get_upstream_info(
        wt.abspath(subpath),
        trust_package=trust,
        net_access=net_access,
        consult_external_directory=consult_external_directory,
        check=check,
    )

    # Compatibility with older upstream-ontologist, which included buildsystem
    # info.
    if isinstance(metadata, tuple):
        buildsystem, unused_reqs, metadata = metadata
    else:
        from ognibuild.buildsystem import get_buildsystem

        buildsystem_subpath, buildsystem = get_buildsystem(
            wt.abspath(subpath))

    try:
        upstream_name = metadata["Name"]
    except KeyError:
        raise UpstreamNameUnknown(wt.abspath(subpath))

    source_name = source_name_from_upstream_name(upstream_name)

    with wt.lock_write():
        with contextlib.ExitStack() as es:
            es.enter_context(ResetOnFailure(
                wt, subpath=subpath, dirty_tracker=dirty_tracker))

            if not wt.has_filename(debian_path):
                wt.mkdir(debian_path)

            from breezy.plugins.debian.upstream.branch import (
                upstream_branch_version,
                upstream_version_add_revision,
            )
            upstream_revision = wt.last_revision()
            upstream_version = upstream_branch_version(
                wt.branch, upstream_revision, upstream_name
            )
            if upstream_version is None and "X-Version" in metadata:
                # They haven't done any releases yet. Assume we're ahead of
                # the next announced release?
                next_upstream_version = debian_upstream_version(metadata["X-Version"])
                upstream_version = upstream_version_add_revision(
                    wt.branch, next_upstream_version, upstream_revision, "~"
                )
            if upstream_version is None:
                upstream_version = upstream_version_add_revision(
                    wt.branch, "0", upstream_revision, "+"
                )
                logging.warning(
                    "Unable to determine upstream version, using %s.",
                    upstream_version)

            if wt.last_revision() == upstream_revision:
                # If at all possible, try to avoid copying
                upstream_tree = wt
            else:
                upstream_tree = wt.revision_tree(upstream_revision)

            if schroot is None:
                session = PlainSession()
            else:
                logging.info('Using schroot %s', schroot)
                session = SchrootSession(schroot)

            with session:
                resolver = auto_resolver(session)
                build_fixers = [InstallFixer(resolver)]

                (pristine_revids, tag_names,
                 upstream_branch_name) = import_upstream_version_from_dist(
                    upstream_tree, os.path.join(subpath, buildsystem_subpath),
                    buildsystem, source_name, upstream_version,
                    session=session, resolver=resolver, fixers=build_fixers,
                    create_dist=create_dist)

                try:
                    upstream_deps = list(buildsystem.get_declared_dependencies(
                        session, build_fixers))
                except NotImplementedError:
                    logging.warning('Unable to obtain declared dependencies.')
                    upstream_deps = None

            if wt.branch.last_revision() != pristine_revids[None]:
                wt.pull(
                    wt.branch, overwrite=True, stop_revision=pristine_revids[None])
                if dirty_tracker:
                    dirty_tracker.mark_clean()

            version = Version(upstream_version + "-1")
            # TODO(jelmer): This is a reasonable guess, but won't always be
            # okay.

            build_deps = []
            test_deps = []
            if upstream_deps:
                from ognibuild.resolver.apt import AptResolver

                with session:
                    apt_resolver = AptResolver.from_session(session)

                    for kind, dep in upstream_deps:
                        apt_dep = apt_resolver.resolve(dep)
                        if apt_dep is None:
                            logging.warning(
                                'Unable to map upstream requirement %s (kind %s) '
                                'to a Debian package', dep, kind)
                            continue
                        logging.debug('Mapped %s (kind: %s) to %s', dep, kind, apt_dep)
                        if kind in ('core', 'build'):
                            build_deps.append(apt_dep)
                        if kind in ('core', 'test', ):
                            test_deps.append(apt_dep)

            if buildsystem:
                try:
                    process = PROCESSORS[buildsystem.name]
                except KeyError:
                    process = process_default
            else:
                process = process_default

            control = process(
                es, wt, subpath, debian_path,
                metadata=metadata, compat_release=compat_release)

            source = control.source

            if not valid_debian_package_name(source['Source']):
                raise SourcePackageNameInvalid(source['Source'])

            for build_dep in build_deps:
                for rel in build_dep.relations:
                    source["Build-Depends"] = ensure_relation(
                        source.get("Build-Depends", ""),
                        PkgRelation.str([rel]))

            if net_access:
                import asyncio

                loop = asyncio.get_event_loop()
                wnpp_bugs = loop.run_until_complete(find_wnpp_bugs(source['Source']))
                if not wnpp_bugs and source['Source'] != upstream_name:
                    wnpp_bugs = loop.run_until_complete(find_wnpp_bugs(upstream_name))
                if not wnpp_bugs:
                    wnpp_bugs = loop.run_until_complete(
                        find_archived_wnpp_bugs(source['Source'])
                    )
                    if wnpp_bugs:
                        logging.warning(
                            "Found archived ITP/RFP bugs for %s: %r", source['Source'], wnpp_bugs
                        )
                    else:
                        logging.warning(
                            "No relevant WNPP bugs found for %s", source['Source'])
                else:
                    logging.info("Found WNPP bugs for %s: %r", source['Source'], wnpp_bugs)
            else:
                wnpp_bugs = None

            write_changelog_template(
                wt.abspath(os.path.join(debian_path, "changelog")),
                source["Source"],
                version,
                wnpp_bugs,
            )

        wt.smart_add([wt.abspath(debian_path)])
        wt.commit(
            "Create debian/ directory",
            allow_pointless=False,
            committer=get_committer(wt),
            reporter=NullCommitReporter(),
        )

    with wt.lock_write():

        lintian_fixers = available_lintian_fixers(force_subprocess=force_subprocess)

        run_lintian_fixers(
            wt,
            lintian_fixers,
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
            diligence=diligence,
        )

        try:
            update_offical_vcs(wt, subpath=subpath, committer=get_committer(wt))
        except NoVcsLocation:
            logging.debug(
                'No public VCS location specified and unable to guess it '
                'based on maintainer e-mail.')

    return DebianizeResult(
        upstream_branch_name=upstream_branch_name,
        tag_names=tag_names,
        upstream_version=upstream_version)


def main(argv=None):
    import argparse
    from breezy.workingtree import WorkingTree

    import breezy  # noqa: E402

    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    parser = argparse.ArgumentParser(prog="debianize")
    parser.add_argument(
        "--directory", "-d",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )
    parser.add_argument(
        "--disable-inotify", action="store_true", default=False, help=argparse.SUPPRESS
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument("--compat-release", type=str, help=argparse.SUPPRESS)
    parser.add_argument(
        "--verbose", help="be verbose", action="store_true", default=False
    )
    parser.add_argument(
        "--disable-net-access",
        help="Do not probe external services.",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--diligent",
        action="count",
        default=0,
        dest="diligence",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--trust",
        action="store_true",
        help="Whether to allow running code from the package.",
    )
    parser.add_argument(
        "--consult-external-directory",
        action="store_true",
        help="Pull in external (not maintained by upstream) directory data",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check guessed metadata against external sources.",
    )
    parser.add_argument(
        "--force-subprocess", action="store_true", help=argparse.SUPPRESS
    )

    args = parser.parse_args(argv)

    logging.basicConfig(level=logging.INFO, format='%(message)s')

    compat_release = args.compat_release
    if compat_release is None:
        import distro_info

        debian_info = distro_info.DebianDistroInfo()
        compat_release = debian_info.stable()

    wt, subpath = WorkingTree.open_containing(args.directory)

    use_inotify = ((False if args.disable_inotify else None),)
    with wt.lock_write():
        try:
            check_clean_tree(wt, wt.basis_tree(), subpath)
        except PendingChanges:
            logging.info("%s: Please commit pending changes first.", wt.basedir)
            return 1

        try:
            debianize(
                wt,
                subpath,
                use_inotify=use_inotify,
                diligence=args.diligence,
                trust=args.trust,
                check=args.check,
                net_access=not args.disable_net_access,
                force_subprocess=args.force_subprocess,
                compat_release=compat_release,
                consult_external_directory=args.consult_external_directory,
                verbose=args.verbose,
            )
        except DebianDirectoryExists as e:
            logging.info(
                "%s: A debian directory already exists. " "Run lintian-brush instead?",
                e.path,
            )
            return 1
        except UpstreamNameUnknown as e:
            logging.info("%s: Unable to determine upstream package name.", e.path)
            if not args.trust:
                logging.info(
                    "Run with --trust if you are okay running code "
                    "from the package?")
            return 1
        except SourcePackageNameInvalid as e:
            logging.info("Unable to sanitize source package name: %s", e.source)
            return 1
        except DistCreationFailed as e:
            logging.fatal('Dist tarball creation failed: %s', e.inner)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
