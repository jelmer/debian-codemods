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

__all__ = [
    'NoBuildToolsFound',
    'debianize',
    ]

import contextlib
from dataclasses import dataclass, field
import errno
from functools import partial
import json
import logging
import os
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
from typing import Optional, Tuple, List, Dict, Any, Callable, cast
from urllib.parse import urlparse


from debian.changelog import Changelog, Version, get_maintainer, format_date
from debmutate.control import (
    ensure_some_version,
    ensure_exact_version,
    ensure_relation,
    ControlEditor,
)
from debian.deb822 import PkgRelation

from breezy import osutils
from breezy.branch import Branch
from breezy.controldir import ControlDir
from breezy.errors import (
    AlreadyBranchError,
    NotBranchError,
    NoSuchRevision,
)
from breezy.commit import NullCommitReporter, PointlessCommit
from breezy.revision import NULL_REVISION
from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from breezy.transport import FileExists

from ognibuild import DetailedFailure, UnidentifiedError
from ognibuild.buildlog import problem_to_upstream_requirement
from ognibuild.buildsystem import (
    get_buildsystem, NoBuildToolsFound, BuildSystem)
from ognibuild.debian.apt import AptManager
from ognibuild.debian.build import DEFAULT_BUILDER
from ognibuild.debian.fix_build import (
    DetailedDebianBuildFailure,
    UnidentifiedDebianBuildError,
    build_incrementally,
    default_fixers,
    )
from ognibuild.dist import (  # noqa: F401
    DistNoTarball,
    create_dist as ogni_create_dist,
    )
from ognibuild.fix_build import iterate_with_build_fixers, BuildFixer

from ognibuild.session import SessionSetupFailure, Session
from ognibuild.session.plain import PlainSession
from ognibuild.session.schroot import SchrootSession
from ognibuild.requirements import (
    Requirement,
    )
from ognibuild.resolver.apt import AptRequirement
from ognibuild.upstream import (
    find_upstream,
    find_apt_upstream,
    go_base_name,
    load_crate_info,
)
from ognibuild.debian.upstream_deps import get_project_wide_deps
from ognibuild.vcs import dupe_vcs_tree

from upstream_ontologist.guess import (
    UpstreamDatum,
    guess_upstream_info,
    summarize_upstream_metadata,
)
from upstream_ontologist.debian import (
    upstream_name_to_debian_source_name as source_name_from_upstream_name,
    upstream_version_to_debian_upstream_version as debian_upstream_version,
    valid_debian_package_name,
)

from breezy.plugins.debian import default_orig_dir
from breezy.plugins.debian.directory import vcs_git_url_to_bzr_url
from breezy.plugins.debian.merge_upstream import (
    get_tarballs,
    do_import,
    get_existing_imported_upstream_revids,
    )
from breezy.plugins.debian.import_dsc import UpstreamAlreadyImported
from breezy.plugins.debian.upstream import PackageVersionNotPresent
from breezy.plugins.debian.upstream.pristinetar import get_pristine_tar_source
from breezy.plugins.debian.upstream.branch import (
    upstream_version_add_revision,
    UpstreamBranchSource,
    DistCommandFailed,
    run_dist_command,
)
from breezy.workspace import (
    check_clean_tree,
    reset_tree,
    WorkspaceDirty,
    )

from buildlog_consultant.common import (
    VcsControlDirectoryNeeded,
    SetuptoolScmVersionIssue,
)

from debmutate.versions import (
    debianize_upstream_version,
)
from debmutate.vcs import unsplit_vcs_url

from . import (
    available_lintian_fixers,
    version_string,
    run_lintian_fixers,
    get_committer,
)
from .debbugs import find_archived_wnpp_bugs, find_wnpp_bugs
from .debhelper import (
    maximum_debhelper_compat_version,
    write_rules_template as write_debhelper_rules_template,
)
from .publish import (
    update_offical_vcs,
    NoVcsLocation,
)
from .standards_version import latest_standards_version


Kickstarter = Callable[[WorkingTree, str], None]


class BuildSystemProcessError(Exception):
    """Error processing buildsystem-specific part of debianization."""

    def __init__(self, buildsystem, message, inner=None):
        self.buildsystem = buildsystem
        self.message = message
        self.inner = inner


class DebianizedPackageRequirementMismatch(Exception):
    """Debianized package does not satisfy requirement."""

    def __init__(self, requirement, control, version, upstream_branch):
        self.requirement = requirement
        self.control = control
        self.version = version
        self.upstream_branch = upstream_branch


class SourcePackageNameInvalid(Exception):
    """Source package name is invalid."""

    def __init__(self, source):
        self.source = source


class SourceNameUnknown(Exception):
    """Unable to determine source name."""

    def __init__(self, upstream_name):
        self.upstream_name = upstream_name


class NoUpstreamReleases(Exception):
    """No upstream releases were found."""

    def __init__(self, upstream_source, name):
        self.upstream_source = upstream_source
        self.name = name


def write_changelog_template(
        path, source_name, version, author=None, wnpp_bugs=None):
    if author is None:
        author = get_maintainer()
    if wnpp_bugs:
        closes = " Closes: " + ", ".join(
            [("#%d" % (bug,)) for bug, kind in wnpp_bugs])
    else:
        closes = ""
    cl = Changelog()
    cl.new_block(
        package=source_name,
        version=version,
        distributions="UNRELEASED",
        urgency="low",
        changes=["", "  * Initial release." + closes, ""],
        author="%s <%s>" % author,
        date=format_date(),
    )
    with open(path, "w") as f:
        f.write(cl.__str__().strip("\n") + "\n")


MINIMUM_CERTAINTY = "possible"  # For now..


def versions_dict():
    import lintian_brush
    import debmutate
    import debian
    import ognibuild
    import buildlog_consultant
    import upstream_ontologist
    return {
        'lintian-brush': lintian_brush.version_string,
        'debmutate': debmutate.version_string,
        'debian': debian.__version__,
        'ognibuild': ognibuild.version_string,
        'buildlog_consultant': buildlog_consultant.version_string,
        'upstream_ontologist': upstream_ontologist.version_string,
    }


class DebianDirectoryExists(Exception):
    """A Debian Directory already exists."""

    def __init__(self, path):
        self.path = path


def go_import_path_from_repo(repo_url):
    parsed_url = urlparse(repo_url)
    p = parsed_url.hostname + parsed_url.path.rstrip('/')
    if p.endswith(".git"):
        p = p[:-4]
    return p


def enable_dh_addon(source, addon):
    source["Build-Depends"] = ensure_some_version(
        source["Build-Depends"], "dh-sequence-%s" % addon
    )


def setup_debhelper(
        wt, debian_path, source, compat_release, addons=None, env=None,
        buildsystem=None):
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


def default_create_dist(
        session, tree, package, version, target_dir, subpath=""):
    try:
        with session:
            try:
                return ogni_create_dist(
                    session, tree, target_dir,
                    include_controldir=False,
                    subdir=(package or "package"),
                    version=version,
                    subpath=subpath)
            except DetailedFailure as e:
                if isinstance(
                        e.error,
                        (VcsControlDirectoryNeeded, SetuptoolScmVersionIssue)):
                    return ogni_create_dist(
                        session, tree, target_dir,
                        include_controldir=True,
                        subdir=(package or "package"),
                        version=version, subpath=subpath)
                else:
                    raise
    except NoBuildToolsFound:
        logging.info(
            "No build tools found, falling back to simple export.")
        return None
    except NotImplementedError:
        logging.info(
            "Build system does not support dist, falling back "
            "to export.")
        return None
    except SessionSetupFailure as exc:
        raise DistCommandFailed(str(exc), 'session-setup-failure') from exc
    except DistNoTarball as e:
        logging.info("Build system did not create a tarball: %s", e)
        return None
    except DetailedFailure as exc:
        raise DistCommandFailed(str(exc), exc.error) from exc
    except UnidentifiedError as exc:
        raise DistCommandFailed(str(exc)) from exc


def import_upstream_version_from_dist(
        wt, subpath, upstream_source, source_name, upstream_version,
        session):
    orig_dir = os.path.abspath(default_orig_dir)

    tag_names = {}
    with TemporaryDirectory() as target_dir:
        locations = upstream_source.fetch_tarballs(
            source_name, upstream_version, target_dir, components=[None])
        if source_name is None:
            source_name = os.path.basename(locations[0]).split('_')[0]
        try:
            tarball_filenames = get_tarballs(
                orig_dir,
                wt, source_name, upstream_version, locations)
        except FileExists as e:
            logging.warning(
                'Tarball %s exists, reusing existing file.', e.path)
            tarball_filenames = [os.path.join(orig_dir, e.path)]
        upstream_revisions = upstream_source\
            .version_as_revisions(source_name, upstream_version)
        files_excluded = None
        try:
            imported_revids = do_import(
                wt, subpath, tarball_filenames, source_name, upstream_version,
                current_version=None,
                upstream_branch=upstream_source.upstream_branch,
                upstream_revisions=upstream_revisions,
                merge_type=None, files_excluded=files_excluded)
        except UpstreamAlreadyImported as e:
            logging.warning(
                'Upstream release %s already imported.',
                e.version)  # type: ignore
            imported_revids = get_existing_imported_upstream_revids(
                upstream_source, source_name, upstream_version)
        pristine_revids = {}
        for (component, tag_name, revid,
             _pristine_tar_imported) in imported_revids:
            pristine_revids[component] = revid
            tag_names[component] = tag_name

    upstream_branch_name = "upstream"
    try:
        branch = wt.controldir.create_branch(upstream_branch_name)
    except AlreadyBranchError:
        logging.info("Upstream branch already exists; not creating.")
    else:
        branch.generate_revision_history(pristine_revids[None])
        logging.info("Created upstream branch.")

    return pristine_revids, tag_names, upstream_branch_name


class ResetOnFailure:

    def __init__(self, wt, subpath=None):
        self.wt = wt
        self.subpath = subpath

    def __enter__(self):
        check_clean_tree(self.wt, self.wt.basis_tree(), self.subpath)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type:
            reset_tree(self.wt, self.wt.basis_tree(), self.subpath)
        return False


def process_setup_py(es, session, wt, subpath, debian_path, upstream_version,
                     metadata, compat_release, buildsystem,
                     buildsystem_subpath, kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        addons=["python3"],
        buildsystem="pybuild")
    source["Testsuite"] = "autopkgtest-pkg-python"
    # TODO(jelmer): check whether project supports python 3
    source["Build-Depends"] = ensure_some_version(
        source["Build-Depends"], "python3-all")
    # TODO(Jelmer): Convert pyproject's build-system.requirements to Python
    # deps
    if buildsystem.build_backend in ("flit.build_api", "flit_core.build_api"):
        source["Build-Depends"] = ensure_some_version(
            source["Build-Depends"], "flit")
        source["Build-Depends"] = ensure_some_version(
            source["Build-Depends"], "python3-toml")
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    # We're going to be running the testsuite as part of the build,
    # so import the test dependencies too.
    import_build_deps(source, test_deps)
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


def process_maven(es, session, wt, subpath, debian_path, upstream_version,
                  metadata, compat_release, buildsystem, buildsystem_subpath,
                  kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(
        ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        buildsystem="maven")
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    upstream_name = metadata['Name']
    source['Source'] = upstream_name
    control.add_binary({
            "Package": "lib%s-java" % upstream_name,
            "Depends": "${java:Depends}",
            "Architecture": "all",
        }
    )
    return control


def process_npm(es, session, wt, subpath, debian_path, upstream_version,
                metadata, compat_release, buildsystem, buildsystem_subpath,
                kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(
        ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release, addons=["nodejs"])
    upstream_name = (
        metadata['Name'].strip('@').replace('/', '-')
        .replace('_', '-').replace('@', '').lower())
    source['Source'] = "node-%s" % upstream_name
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    control.add_binary(
        {"Package": "node-%s" % upstream_name, "Architecture": "all"})
    source["Testsuite"] = "autopkgtest-pkg-nodejs"
    return control


def perl_package_name(upstream_name):
    if upstream_name.startswith('lib'):
        upstream_name = upstream_name[len('lib'):]
    return ("lib%s-perl" % upstream_name.replace('::', '-')
            .replace('_', '').lower())


def process_dist_zilla(es, session, wt, subpath, debian_path, upstream_version,
                       metadata, compat_release, buildsystem,
                       buildsystem_subpath, kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(
        ControlEditor.create(wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source['Source'] = perl_package_name(upstream_name)
    source["Rules-Requires-Root"] = "no"
    source['Testsuite'] = 'autopkgtest-pkg-perl'
    source["Standards-Version"] = latest_standards_version()
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release,
        addons=["dist-zilla"])
    control.add_binary(
        {"Package": source['Source'],
         "Depends": "${perl:Depends}",
         "Architecture": "all"
         })
    return control


def process_makefile_pl(es, session, wt, subpath, debian_path,
                        upstream_version, metadata, compat_release,
                        buildsystem, buildsystem_subpath, kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source['Source'] = perl_package_name(upstream_name)
    source["Rules-Requires-Root"] = "no"
    source['Testsuite'] = 'autopkgtest-pkg-perl'
    source["Standards-Version"] = latest_standards_version()
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(wt, debian_path, source, compat_release=compat_release)
    control.add_binary(
        {"Package": source['Source'],
         "Depends": "${perl:Depends}",
         "Architecture": "all"
         })
    return control


def process_perl_build_tiny(es, session, wt, subpath, debian_path,
                            upstream_version, metadata, compat_release,
                            buildsystem, buildsystem_subpath,
                            kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source['Source'] = perl_package_name(upstream_name)
    source["Rules-Requires-Root"] = "no"
    source['Testsuite'] = 'autopkgtest-pkg-perl'
    source["Standards-Version"] = latest_standards_version()
    source["Build-Depends"] = "libmodule-build-perl"
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(wt, debian_path, source, compat_release=compat_release)
    control.add_binary(
        {"Package": source['Source'],
         "Depends": "${perl:Depends}",
         "Architecture": "all"
         })
    return control


def process_golang(es, session, wt, subpath, debian_path, upstream_version,
                   metadata, compat_release, buildsystem, buildsystem_subpath,
                   kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    source["XS-Go-Import-Path"] = go_import_path_from_repo(
        metadata["Repository"]
    )
    if "Repository-Browse" in metadata:
        source["Homepage"] = metadata["Repository-Browse"]
    source["Section"] = "devel"
    parsed_url = urlparse(metadata["Repository"])
    godebname = go_base_name(parsed_url.hostname + parsed_url.path)
    source['Source'] = "golang-%s" % godebname
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
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


def process_r(es, session, wt, subpath, debian_path,
              upstream_version, metadata, compat_release, buildsystem,
              buildsystem_subpath, kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source

    if metadata.get('Archive') == 'CRAN':
        archive = 'cran'
    elif metadata.get('Archive') == 'Bioconductor':
        archive = 'bioc'
    else:
        archive = 'other'

    source["Source"] = "r-{}-{}".format(archive, metadata['Name'].lower())
    source["Rules-Requires-Root"] = "no"
    source["Build-Depends"] = "dh-r, r-base-dev"
    source["Standards-Version"] = latest_standards_version()
    source["Testsuite"] = "autopkgtest-pkg-r"
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(
        wt, debian_path, source, compat_release=compat_release,
        buildsystem="R")
    # For now, just assume a single binary package that is
    # architecture-dependent.
    control.add_binary({
        "Package": "r-{}-{}".format(archive, metadata['Name'].lower()),
        "Architecture": 'any',
        'Depends': '${R:Depends}, ${shlibs:Depends}, ${misc:Depends}',
        'Recommends': '${R:Recommends}',
        'Suggests': '${R:Suggests}',
        })
    return control


def process_octave(es, session, wt, subpath, debian_path, metadata,
                   upstream_version, compat_release, buildsystem,
                   buildsystem_subpath, kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source

    source["Source"] = "octave-%s" % metadata['Name'].lower()
    source["Rules-Requires-Root"] = "no"
    source["Build-Depends"] = "dh-octave"
    source["Standards-Version"] = latest_standards_version()
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(
        wt, debian_path, source, compat_release=compat_release,
        buildsystem="octave", addons=['octave'])
    # For now, just assume a single binary package that is
    # architecture-independent.
    control.add_binary({
        "Package": "octave-%s" % metadata['Name'].lower(),
        "Architecture": 'all',
        'Depends': '${octave:Depends}, ${misc:Depends}',
        'Description': '${octave:Upstream-Description}',
        })
    return control


def process_default(es, session, wt, subpath, debian_path, upstream_version,
                    metadata, compat_release, buildsystem, buildsystem_subpath,
                    kickstart_from_dist):
    kickstart_from_dist(wt, subpath)
    control = es.enter_context(ControlEditor.create(
        wt.abspath(os.path.join(debian_path, 'control'))))
    source = control.source
    upstream_name = metadata['Name']
    source_name = source_name_from_upstream_name(upstream_name)
    if source_name is None:
        raise SourceNameUnknown(upstream_name)
    source["Source"] = source_name
    source["Rules-Requires-Root"] = "no"
    source["Standards-Version"] = latest_standards_version()
    build_deps, test_deps = get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath)
    import_build_deps(source, build_deps)
    setup_debhelper(
        wt, debian_path,
        source, compat_release=compat_release)
    # For now, just assume a single binary package that is
    # architecture-dependent.
    for binary_name, arch in [(source['Source'], "any")]:
        control.add_binary({"Package": binary_name, "Architecture": arch})
    return control


def process_cargo(es, session, wt, subpath, debian_path, upstream_version,
                  metadata, compat_release, buildsystem, buildsystem_subpath,
                  kickstart_from_dist):
    wt.branch.generate_revision_history(NULL_REVISION)
    reset_tree(wt, wt.basis_tree(), subpath)
    from debmutate.debcargo import (
        DebcargoControlShimEditor, unmangle_debcargo_version)
    crate = metadata.get('X-Cargo-Crate')
    if crate is None:
        crate = metadata['Name'].replace('_', '-')
    if not wt.has_filename(debian_path):
        wt.mkdir(debian_path)
    if not wt.is_versioned(debian_path):
        wt.add(debian_path)
    # Only set semver_suffix if this is not the latest version
    import semver
    try:
        desired_version = semver.VersionInfo.parse(upstream_version)
    except ValueError as exc:
        raise BuildSystemProcessError(buildsystem, str(exc), exc) from exc
    data = load_crate_info(crate)
    if data is None:
        raise BuildSystemProcessError(
            buildsystem, 'Crate does not exist' % crate)
    features = None
    crate_version = None
    for version_info in data['versions']:
        available_version = semver.VersionInfo.parse(version_info['num'])
        if ((available_version.major, available_version.minor)
                > (desired_version.major, desired_version.minor)):
            semver_suffix = True
            break
        if unmangle_debcargo_version(upstream_version) == version_info['num']:
            crate_version = version_info['num']
            features = list(version_info['features'])
    else:
        semver_suffix = False
    control = es.enter_context(DebcargoControlShimEditor.from_debian_dir(
        wt.abspath(debian_path), crate, crate_version, features))
    control.debcargo_editor['semver_suffix'] = semver_suffix
    control.debcargo_editor['overlay'] = '.'
    return control


Processor = Callable[
    [contextlib.ExitStack, Session, WorkingTree, str, str, str,
     Dict[str, Any], Optional[str], BuildSystem, str, Kickstarter],
    ControlEditor]


PROCESSORS: Dict[str, Processor] = {
    "setup.py": process_setup_py,
    "npm": process_npm,
    "maven": process_maven,
    "dist-zilla": process_dist_zilla,
    "dist-inkt": process_dist_zilla,
    "perl-build-tiny": process_perl_build_tiny,
    "makefile.pl": process_makefile_pl,
    "cargo": process_cargo,
    "golang": process_golang,
    "R": process_r,
    "octave": process_octave,
}


def source_name_from_directory_name(path):
    d = os.path.basename(path)
    if '-' in d:
        parts = d.split('-')
        if parts[-1][0].isdigit():
            return '-'.join(parts[:-1])
    return d


@dataclass
class DebianizeResult:
    """Debianize result."""

    upstream_branch_name: Optional[str] = None
    tag_names: Dict[Optional[str], str] = field(default_factory=dict)
    upstream_version: Optional[str] = None
    wnpp_bugs: List[Tuple[int, str]] = field(default_factory=list)
    vcs_url: Optional[str] = None


def import_build_deps(source, build_deps):
    for build_dep in build_deps:
        for rel in build_dep.relations:
            source["Build-Depends"] = ensure_relation(
                source.get("Build-Depends", ""),
                PkgRelation.str([rel]))


def import_upstream_dist(
        pristine_tar_source, wt, upstream_source, subpath, source_name,
        upstream_version, session):
    if pristine_tar_source.has_version(
            source_name, upstream_version, try_hard=False):
        logging.warning(
            'Upstream version %s/%s already imported.',
            source_name, upstream_version)
        pristine_revids = pristine_tar_source\
            .version_as_revisions(source_name, upstream_version)
        upstream_branch_name = None
        tag_names = {}
    else:
        (pristine_revids, tag_names,
         upstream_branch_name) = import_upstream_version_from_dist(
            wt, subpath,
            upstream_source,
            source_name, upstream_version,
            session=session)

    return pristine_revids[None], upstream_branch_name, tag_names


def generic_get_source_name(wt, subpath, metadata):
    try:
        source_name = source_name_from_upstream_name(metadata['Name'])
    except KeyError:
        source_name = None
    else:
        if not valid_debian_package_name(source_name):
            source_name = None
    if source_name is None:
        source_name = source_name_from_directory_name(wt.abspath(subpath))
        if not valid_debian_package_name(source_name):
            source_name = None
    return source_name


def get_upstream_version(
        upstream_source, metadata: Dict[str, Any],
        local_dir=None,
        upstream_subpath: Optional[str] = None,
        upstream_version: Optional[str] = None) -> Tuple[str, str]:
    # TODO(jelmer): if upstream_subpath != "", perhaps ignore info from
    # upstream_source?
    if upstream_version is None:
        upstream_version, mangled_upstream_version = (
            upstream_source.get_latest_version(metadata.get("Name"), None))
    else:
        mangled_upstream_version = debianize_upstream_version(upstream_version)
    if upstream_version is None:
        raise NoUpstreamReleases(upstream_source, metadata.get("Name"))

    upstream_revision = upstream_source.version_as_revision(
        metadata.get("Name"), mangled_upstream_version)

    if upstream_version is None and "X-Version" in metadata:
        # They haven't done any releases yet. Assume we're ahead of
        # the next announced release?
        next_upstream_version = debian_upstream_version(metadata["X-Version"])
        upstream_version = upstream_version_add_revision(
            upstream_source.upstream_branch, next_upstream_version,
            upstream_revision, "~"
        )
    if upstream_version is None:
        upstream_version = upstream_version_add_revision(
            upstream_source.upstream_branch, "0", upstream_revision, "+"
        )
        logging.warning(
            "Unable to determine upstream version, using %s.",
            upstream_version)
    return upstream_version, mangled_upstream_version


def find_wnpp_bugs_harder(source_name, upstream_name):
    wnpp_bugs = find_wnpp_bugs(source_name)
    if not wnpp_bugs and source_name != upstream_name:
        wnpp_bugs = find_wnpp_bugs(upstream_name)
    if not wnpp_bugs:
        wnpp_bugs = find_archived_wnpp_bugs(source_name)
        if wnpp_bugs:
            logging.warning(
                "Found archived ITP/RFP bugs for %s: %r",
                source_name, [bug for bug, kind in wnpp_bugs]
            )
        else:
            logging.warning(
                "No relevant WNPP bugs found for %s", source_name)
    else:
        logging.info(
            "Found WNPP bugs for %s: %r",
            source_name, [bug for bug, kind in wnpp_bugs])

    return wnpp_bugs


def debianize(  # noqa: C901
    wt: WorkingTree, subpath: str,
    *,
    upstream_branch: Optional[Branch], upstream_subpath: Optional[str],
    use_inotify: Optional[bool] = None,
    diligence: int = 0,
    trust: bool = False,
    check: bool = False,
    net_access: bool = True,
    force_subprocess: bool = False,
    force_new_directory: bool = False,
    compat_release: Optional[str] = None,
    minimum_certainty: str = MINIMUM_CERTAINTY,
    consult_external_directory: bool = True,
    verbose: bool = False,
    schroot: Optional[str] = None,
    unshare: Optional[str] = None,
    create_dist=None,
    committer: Optional[str] = None,
    upstream_version_kind: str = "auto",
    debian_revision: str = "1",
    upstream_version: Optional[str] = None,
    requirement: Optional[Requirement] = None,
    team: Optional[str] = None,
    buildsystem_name: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None
):
    if committer is None:
        committer = get_committer(wt)

    debian_path = osutils.pathjoin(subpath, "debian")
    if (wt.has_filename(debian_path)
            and list(os.listdir(wt.abspath(debian_path)))
            and not force_new_directory):
        raise DebianDirectoryExists(wt.abspath(subpath))

    metadata_items: List[UpstreamDatum] = []
    if metadata is None:
        metadata = {}
    else:
        metadata = dict(metadata)

    def import_metadata_from_path(p):
        metadata_items.extend(guess_upstream_info(p, trust_package=trust))
        assert isinstance(metadata, dict)
        metadata.update(
            summarize_upstream_metadata(
                metadata_items, p, net_access=net_access,
                consult_external_directory=consult_external_directory,
                check=check))

    if not verbose:
        commit_reporter = NullCommitReporter()
    else:
        commit_reporter = None

    result = DebianizeResult()

    with wt.lock_write():
        with contextlib.ExitStack() as es:
            es.enter_context(ResetOnFailure(wt, subpath=subpath))

            if not wt.has_filename(debian_path):
                wt.mkdir(debian_path)
            if not wt.is_versioned(debian_path):
                wt.add(debian_path)

            session: Session
            if schroot:
                logging.info('Using schroot %s', schroot)
                session = SchrootSession(schroot)
            elif unshare:
                logging.info('Using tarball %s for unshare', unshare)
                from ognibuild.session.unshare import (
                    UnshareSession)  # type: ignore
                session = UnshareSession.from_tarball(unshare)
            else:
                session = PlainSession()

            if upstream_branch:
                upstream_source = UpstreamBranchSource.from_branch(
                    upstream_branch, version_kind=upstream_version_kind,
                    local_dir=wt.controldir,
                    create_dist=(
                        create_dist or partial(default_create_dist, session)))
            else:
                upstream_source = None

            if upstream_version is not None:
                mangled_upstream_version = debianize_upstream_version(
                    upstream_version)
            else:
                upstream_version, mangled_upstream_version = (
                    get_upstream_version(
                        upstream_source, metadata, local_dir=wt.controldir,
                        upstream_subpath=upstream_subpath,
                        upstream_version=upstream_version))

            result.upstream_version = upstream_version

            source_name = generic_get_source_name(wt, subpath, metadata)

            def kickstart_from_dist(wt, subpath):
                logging.info(
                    "Kickstarting from dist tarball. "
                    "Using upstream version %s", mangled_upstream_version)

                pristine_tar_source = get_pristine_tar_source(wt, wt.branch)
                (upstream_dist_revid, result.upstream_branch_name,
                 result.tag_names) = import_upstream_dist(
                    pristine_tar_source, wt, upstream_source, upstream_subpath,
                    source_name, mangled_upstream_version, session)

                if wt.branch.last_revision() != upstream_dist_revid:
                    wt.pull(
                        upstream_source.upstream_branch, overwrite=True,
                        stop_revision=upstream_dist_revid)

                    # Gather metadata items again now that we're at the correct
                    # revision
                    import_metadata_from_path(wt.abspath(subpath))

                if wt.has_filename(debian_path) and force_new_directory:
                    shutil.rmtree(wt.abspath(debian_path))
                    wt.mkdir(wt.abspath(debian_path))
                    with contextlib.suppress(PointlessCommit):
                        wt.commit(
                            'Remove old debian directory',
                            specific_files=[debian_path],
                            reporter=NullCommitReporter())

                wt.mkdir(os.path.join(debian_path, 'source'))
                wt.add(os.path.join(debian_path, 'source'))
                wt.put_file_bytes_non_atomic(
                    os.path.join(debian_path, 'source', 'format'),
                    b'3.0 (quilt)\n')

            if upstream_source:
                try:
                    (upstream_vcs_tree,
                     upstream_vcs_subpath) = upstream_source.revision_tree(
                        source_name, mangled_upstream_version)
                except (PackageVersionNotPresent, NoSuchRevision):
                    logging.warning(
                        'Unable to find upstream version %s/%s '
                        'in upstream source %r. Unable to extract metadata.',
                        source_name, mangled_upstream_version, upstream_source)
                    exported_upstream_tree_path = None
                else:
                    assert upstream_vcs_subpath == ''
                    # TODO(jelmer): Don't export, just access from memory.
                    exported_upstream_tree_path = es.enter_context(
                        TemporaryDirectory())
                    dupe_vcs_tree(
                        upstream_vcs_tree, exported_upstream_tree_path)
                    exported_upstream_tree_subpath = os.path.join(
                        exported_upstream_tree_path, upstream_subpath)
                    if not os.path.isdir(exported_upstream_tree_subpath):
                        raise Exception(
                            f'subdirectory {upstream_subpath} does not '
                            f'exist in upstream version {upstream_version}')
                    import_metadata_from_path(exported_upstream_tree_subpath)

            if (buildsystem_name is None
                    and exported_upstream_tree_path is not None):
                buildsystem_subpath, buildsystem = get_buildsystem(
                    os.path.join(exported_upstream_tree_path, subpath))
                if buildsystem:
                    buildsystem_name = buildsystem.name
            else:
                buildsystem = None
                buildsystem_subpath = ''

            if buildsystem_name:
                try:
                    process = PROCESSORS[buildsystem_name]
                except KeyError:
                    logging.warning(
                        'No support in debianize for build system %s, '
                        'falling back to default.',
                        buildsystem_name)
                    process = process_default
            else:
                process = process_default

            logging.info('Creating core packaging using %s', process.__name__)

            os.chdir(wt.abspath(subpath))

            control = process(
                es, session,
                wt, subpath,
                debian_path,
                upstream_version,
                metadata, compat_release,
                buildsystem,
                buildsystem_subpath,
                kickstart_from_dist)

            source = control.source

            if team:
                control.source['Maintainer'] = team
                uploader = '%s <%s>' % get_maintainer()
                if uploader != team:
                    control.source['Uploaders'] = uploader

            if not valid_debian_package_name(source['Source']):
                raise SourcePackageNameInvalid(source['Source'])

            if net_access:
                wnpp_bugs = find_wnpp_bugs_harder(
                    source['Source'], metadata.get('Name'))
            else:
                wnpp_bugs = None

            result.wnpp_bugs = wnpp_bugs

            version = Version(mangled_upstream_version + "-" + debian_revision)
            write_changelog_template(
                wt.abspath(os.path.join(debian_path, "changelog")),
                source["Source"],
                version,
                get_maintainer(),
                wnpp_bugs,
            )

            if (requirement and requirement.family == 'apt' and
                    not cast(AptRequirement, requirement).satisfied_by(
                        control.binaries, version)):
                logging.warning(
                    'Debianized package (binary packages: %r), version %s '
                    'did not satisfy requirement %r. '
                    'Wrong repository (%s)?',
                    [binary['Package'] for binary in control.binaries],
                    version, requirement, upstream_branch)
                raise DebianizedPackageRequirementMismatch(
                    requirement, control, version,
                    upstream_branch)

            control.wrap_and_sort()
            control.sort_binary_packages()

        wt.smart_add([wt.abspath(debian_path)])
        wt.commit(
            "Create debian/ directory",
            allow_pointless=False,
            committer=committer,
            reporter=commit_reporter,
        )

    with wt.lock_write():

        lintian_fixers = available_lintian_fixers(
            force_subprocess=force_subprocess)

        run_lintian_fixers(
            wt,
            list(lintian_fixers),
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
            result.vcs_url = unsplit_vcs_url(*update_offical_vcs(
                wt, subpath=subpath, committer=committer))
        except NoVcsLocation:
            logging.debug(
                'No public VCS location specified and unable to guess it '
                'based on maintainer e-mail.')
        except FileNotFoundError:
            logging.info(
                'No control file or debcargo.toml file, '
                'not setting vcs information.')

    return result


class SimpleTrustedAptRepo:

    def __init__(self, directory):
        self.directory = directory
        self.httpd = None
        self.thread = None

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()
        return False

    def sources_lines(self):
        if not self.httpd:
            raise RuntimeError("httpd not yet started")
        if os.path.exists(os.path.join(self.directory, 'Packages.gz')):
            return [
                "deb [trusted=yes] http://%s:%d/ ./" % (
                    self.httpd.server_name, self.httpd.server_port)]
        else:
            return []

    def start(self):
        if self.thread is not None:
            raise RuntimeError('thread already active')
        import http.server
        from threading import Thread
        hostname = "localhost"

        class RequestHandler(http.server.SimpleHTTPRequestHandler):

            def log_message(self, format, *args):
                return

        handler = partial(RequestHandler, directory=self.directory)
        self.httpd = http.server.HTTPServer((hostname, 0), handler, False)
        self.httpd.server_bind()
        logging.info(
            'Local apt repo started at http://%s:%d/',
            self.httpd.server_name, self.httpd.server_port)
        self.httpd.server_activate()

        def serve_forever(httpd):
            with httpd:  # to make sure httpd.server_close is called
                httpd.serve_forever()

        self.thread = Thread(
            target=serve_forever, args=(self.httpd, ), daemon=True)
        self.thread.start()

    def stop(self):
        if self.httpd is not None:
            self.httpd.shutdown()
            self.httpd = None
        if self.thread is not None:
            self.thread.join()
            self.thread = None

    def refresh(self):
        import gzip
        packages = subprocess.check_output(
            ['dpkg-scanpackages', '-m', '.', '/dev/null'],
            cwd=self.directory)
        with gzip.GzipFile(
                os.path.join(self.directory, 'Packages.gz'), 'wb') as f:
            f.write(packages)


def use_packaging_branch(wt: WorkingTree, branch_name: str) -> None:
    last_revision = wt.last_revision()
    try:
        target_branch = wt.controldir.open_branch(branch_name)
    except NotBranchError:
        target_branch = wt.controldir.create_branch(branch_name)

    target_branch.generate_revision_history(last_revision)
    logging.info('Switching to packaging branch %s.', branch_name)
    wt.controldir.set_branch_reference(target_branch, name="")
    # TODO(jelmer): breezy bug?
    wt._branch = target_branch


class DebianizeFixer(BuildFixer):
    """Fixer that invokes debianize to create a package."""

    def __str__(self):
        return "debianize fixer"

    def __repr__(self):
        return "{}({!r}, {!r})".format(
            type(self).__name__, self.vcs_directory,
            self.apt_repo)

    def __init__(self, vcs_directory, apt_repo, do_build, diligence=0,
                 trust=False, check=True, net_access=True,
                 force_new_directory=False,
                 team=None, verbose=False, force_subprocess=False,
                 upstream_version_kind="auto",
                 debian_revision="1",
                 schroot=None,
                 unshare=None,
                 consult_external_directory=True,
                 use_inotify=None,
                 create_dist=None,
                 compat_release=None):
        self.vcs_directory = vcs_directory
        self.apt_repo = apt_repo
        self.diligence = diligence
        self.trust = trust
        self.check = check
        self.net_access = net_access
        self.force_new_directory = force_new_directory
        self.team = team
        self.verbose = verbose
        self.force_subprocess = force_subprocess
        self.upstream_version_kind = upstream_version_kind
        self.debian_revision = debian_revision
        self.consult_external_directory = consult_external_directory
        self.schroot = schroot
        self.unshare = unshare
        self.use_inotify = use_inotify
        self.create_dist = create_dist
        self.compat_release = compat_release
        self.do_build = do_build

    def can_fix(self, problem):
        requirement = problem_to_upstream_requirement(problem)
        if requirement is None:
            return False
        return find_upstream(requirement) is not None

    def _fix(self, problem, phase: Tuple[str, ...]):
        requirement = problem_to_upstream_requirement(problem)
        if requirement is None:
            return False
        logging.debug(
            'Translated problem %r to requirement %r', problem, requirement)
        upstream_info = find_upstream(requirement)
        if upstream_info is None:
            logging.error(
                'Unable to find upstream information for requirement %r',
                requirement)
            return False
        if upstream_info.branch_url:
            logging.info(
                'Packaging %r to address %r',
                upstream_info.branch_url, problem)
            try:
                upstream_branch = Branch.open(upstream_info.branch_url)
            except NotBranchError as e:
                logging.warning(
                    'Unable to open branch %s: %r', upstream_info.branch_url,
                    e)
                upstream_branch = None
        else:
            upstream_branch = None
        if upstream_info.name is not None:
            vcs_path = os.path.join(
                self.vcs_directory,
                upstream_info.name.replace('/', '-'))
        else:
            raise AssertionError('no upstream name provided')
        if os.path.exists(vcs_path):
            shutil.rmtree(vcs_path)
        if upstream_branch:
            format = upstream_branch.controldir.cloning_metadir()
        else:
            # TODO(jelmer): default to git?
            format = None
        result = ControlDir.create_branch_convenience(
            vcs_path, force_new_tree=True,
            format=format)
        new_wt = result.controldir.open_workingtree()
        new_subpath = ''
        debianize(
            new_wt, new_subpath,
            upstream_branch=upstream_branch,
            upstream_subpath=(upstream_info.branch_subpath or ''),
            use_inotify=self.use_inotify,
            diligence=self.diligence,
            create_dist=self.create_dist,
            trust=self.trust,
            check=self.check,
            net_access=self.net_access,
            force_new_directory=self.force_new_directory,
            force_subprocess=self.force_subprocess,
            compat_release=self.compat_release,
            consult_external_directory=self.consult_external_directory,
            verbose=self.verbose, schroot=self.schroot,
            unshare=self.unshare,
            debian_revision=self.debian_revision,
            upstream_version=upstream_info.version,
            upstream_version_kind=self.upstream_version_kind,
            requirement=requirement,
            buildsystem_name=upstream_info.buildsystem,
            team=self.team,
            metadata=upstream_info.metadata)
        self.do_build(
            new_wt, new_subpath, self.apt_repo.directory,
            extra_repositories=self.apt_repo.sources_lines())
        self.apt_repo.refresh()
        return True


def report_fatal(code, description, hint=None, details=None):
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'versions': versions_dict(),
                'description': description,
                'details': details}, f)
    logging.fatal('%s', description)
    if hint:
        logging.info('%s', hint)


def default_debianize_cache_dir():
    from xdg.BaseDirectory import xdg_cache_home
    cache_dir = os.path.join(xdg_cache_home, 'debianize')
    os.makedirs(cache_dir, exist_ok=True)
    return cache_dir


def main(argv=None):  # noqa: C901
    import argparse

    import breezy
    breezy.initialize()  # type: ignore
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
        "--disable-inotify", action="store_true", default=False,
        help=argparse.SUPPRESS
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument(
        "--compat-release", type=str, help=argparse.SUPPRESS,
        default=os.environ.get('DEB_COMPAT_RELEASE'))
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
    parser.add_argument(
        "--force-new-directory", action="store_true",
        help="Create a new debian/ directory even if one already exists.")
    parser.add_argument(
        "--iterate-fix", "-x", action="store_true",
        help="Invoke deb-fix-build afterwards to build package and add "
        "missing dependencies.")
    parser.add_argument(
        '--install', '-i', action='store_true',
        help='Install package after building (implies --iterate-fix)')
    parser.add_argument(
        "--schroot", type=str,
        help="Schroot to use for building and apt archive access")
    parser.add_argument(
        "--unshare", type=str,
        help="Unshare tarball to use for building and apt archive access")
    parser.add_argument(
        "--build-command",
        type=str,
        help="Build command (used for --iterate-fix)",
        default=(DEFAULT_BUILDER + " -A -s -v"),
    )
    parser.add_argument(
        "--max-build-iterations",
        type=int,
        default=50,
        help=argparse.SUPPRESS)
    parser.add_argument(
        '--release', dest='upstream-version-kind', const="release",
        action='store_const',
        help='Package latest upstream release rather than a snapshot.')
    parser.add_argument(
        '--upstream-version-kind', choices=['auto', 'release', 'snapshot'],
        default='auto',
        help="What kind of release to package.")
    parser.add_argument(
        "--recursive", "-r",
        action="store_true",
        help="Attempt to package dependencies if they are not yet packaged.")
    parser.add_argument(
        '--output-directory',
        type=str,
        help='Output directory.')
    parser.add_argument(
        '--discard-output',
        action='store_true',
        help='Store output in a temporary directory (just test).')
    parser.add_argument(
        '--debian-revision',
        type=str,
        default='1',
        help='Debian revision for the new release.')
    parser.add_argument(
        '--upstream-version',
        type=str,
        help='Upstream version to package.')
    parser.add_argument(
        '--dist-command', type=str,
        help='Dist command', default=os.environ.get('DIST'))
    parser.add_argument(
        '--team', type=str,
        help='Maintainer team ("$NAME <$EMAIL>")')
    parser.add_argument(
        '--debian-branch', type=str,
        help=('Name of Debian branch to create. Empty string to stay at '
              'current branch.'),
        default='%(vendor)s/main')
    parser.add_argument(
        '--debian-binary', type=str,
        help=(
            'Package whatever source will create the named Debian '
            'binary package.'))
    parser.add_argument(
        '--log-directory',
        type=str,
        default=os.environ.get('LOG_DIRECTORY'),
        help='Directory to write log files to.')
    parser.add_argument(
        "--dep-server-url", type=str,
        help="ognibuild dep server to use",
        default=os.environ.get('OGNIBUILD_DEPS'))
    parser.add_argument('upstream', nargs='?', type=str)

    args = parser.parse_args(argv)

    if args.verbose:
        loglevel = logging.DEBUG
    else:
        loglevel = logging.INFO
    logging.basicConfig(level=loglevel, format='%(message)s')

    logging.warning(
        'debianize is experimental and often generates packaging '
        'that is incomplete or does not build as-is. '
        'If you encounter issues, please consider filing a bug.')

    compat_release = args.compat_release
    if compat_release is None:
        import distro_info

        debian_info = distro_info.DebianDistroInfo()
        compat_release = debian_info.stable()

    try:
        wt, subpath = WorkingTree.open_containing(args.directory)
    except NotBranchError as e:
        logging.fatal(
            'please run debianize in an existing branch where '
            'it should add the packaging: %s', e)
        return 1

    create_dist: Optional[Callable[[Tree, str, str, str], str]]
    if args.dist_command:
        def create_dist(tree, package, version, target_dir, subpath=""):
            return run_dist_command(
                tree, package, version, target_dir, args.dist_command,
                subpath=subpath)
    else:
        create_dist = None

    metadata = {}

    # For now...
    if args.upstream:
        try:
            upstream_branch, upstream_subpath = Branch.open_containing(
                args.upstream)
        except NotBranchError as e:
            logging.fatal('%s: not a valid branch: %s', args.upstream, e)
            return 1
        metadata['Repository'] = UpstreamDatum(
            'Repository', args.upstream, certainty='confident')
    elif args.debian_binary:
        apt_requirement = AptRequirement.from_str(args.debian_binary)
        upstream_info = find_apt_upstream(apt_requirement)
        if not upstream_info:
            logging.fatal(
                '%s: Unable to find upstream info for %s', args.debian_binary,
                apt_requirement)
            return 1
        logging.info(
            'Found relevant upstream branch at %s', upstream_info.branch_url)
        upstream_branch = Branch.open(upstream_info.branch_url)
        upstream_subpath = upstream_info.branch_subpath

        metadata['Repository'] = UpstreamDatum(
            'Repository', upstream_info.branch_url, certainty='confident')
    else:
        if wt.has_filename(os.path.join(subpath, 'debian')):
            report_fatal(
                code='debian-directory-exists',
                description=(
                    f"{wt.abspath(subpath)}: "
                    "A debian directory already exists."),
                hint=("Run lintian-brush instead or "
                      "specify --force-new-directory."),
            )
            return 1
        logging.info(
            'No upstream repository specified, using upstream source in %s',
            wt.abspath(subpath))
        upstream_branch = wt.branch
        upstream_subpath = subpath

    if args.debian_branch:
        from debmutate.vendor import get_vendor_name

        use_packaging_branch(
            wt, args.debian_branch % {'vendor': get_vendor_name().lower()})

    use_inotify = (False if args.disable_inotify else None)
    with wt.lock_write():
        try:
            debianize_result = debianize(
                wt, subpath,
                upstream_branch=upstream_branch,
                upstream_subpath=upstream_subpath,
                use_inotify=use_inotify,
                diligence=args.diligence,
                trust=args.trust,
                check=args.check,
                net_access=not args.disable_net_access,
                force_new_directory=args.force_new_directory,
                force_subprocess=args.force_subprocess,
                compat_release=compat_release,
                consult_external_directory=args.consult_external_directory,
                verbose=args.verbose,
                schroot=args.schroot,
                unshare=args.unshare,
                upstream_version_kind=args.upstream_version_kind,
                debian_revision=args.debian_revision,
                create_dist=create_dist,
                upstream_version=args.upstream_version,
                metadata=metadata,
            )
        except PackageVersionNotPresent:
            if args.upstream_version:
                report_fatal(
                    'requested-version-missing',
                    'Requested version %s not present upstream' %
                    args.upstream_version)
                return 1
            else:
                # For now
                raise
        except DistCommandFailed as e:
            report_fatal(
                e.kind or "dist-command-failed", e.error)  # type: ignore
            return 1
        except WorkspaceDirty:
            report_fatal(
                "pending-changes", "Please commit pending changes first.")
            return 1
        except DebianDirectoryExists as e:
            report_fatal(
                code='debian-directory-exists',
                description="%s: A debian directory already exists. " % e.path,
                hint=("Run lintian-brush instead or "
                      "specify --force-new-directory."),
            )
            return 1
        except SourcePackageNameInvalid as e:
            report_fatal(
                code='invalid-source-package-name',
                description=(
                    "Generated source package name %r is not valid."
                    % e.source))
            return 1
        except NoUpstreamReleases:
            report_fatal(
                'no-upstream-releases',
                'The upstream project does not appear to have '
                'made any releases.')
        except NoBuildToolsFound:
            report_fatal(
                'no-build-tools',
                "Unable to find any build systems in upstream sources.")
            return 1
        except DetailedFailure as e:
            report_fatal(
                'debianize-' + e.error.kind,
                str(e),
                details=(e.error.json() if e.error else None))
            return 1
        except BuildSystemProcessError as e:
            report_fatal(
                'build-system-process-error',
                e.message)
            return 1
        except OSError as e:
            if e.errno == errno.ENOSPC:
                report_fatal('no-space-on-device', str(e))
                return 1
            else:
                raise

    if args.install:
        args.iterate_fix = True

    if args.iterate_fix:
        session: Session

        if args.schroot:
            logging.info('Using schroot %s', args.schroot)
            session = SchrootSession(args.schroot)
        elif args.unshare:
            logging.info('Using tarball %s for unshare', args.unshare)
            from ognibuild.session.unshare import (
                UnshareSession)  # type: ignore
            session = UnshareSession.from_tarball(args.unshare)
        else:
            session = PlainSession()

        with contextlib.ExitStack() as es:
            es.enter_context(session)
            apt = AptManager.from_session(session)
            if args.discard_output:
                args.output_directory = es.enter_context(TemporaryDirectory())
            if not args.output_directory:
                args.output_directory = default_debianize_cache_dir()
                logging.info(
                    'Building dependencies in %s', args.output_directory)

            def do_build(
                    wt, subpath, incoming_directory, extra_repositories=None):
                fixers = default_fixers(
                    wt, subpath, apt,
                    update_changelog=False, committer=None,
                    dep_server_url=args.dep_server_url)
                return build_incrementally(
                    local_tree=wt,
                    suffix=None,
                    build_suite=None,
                    fixers=fixers,
                    output_directory=incoming_directory,
                    build_command=args.build_command,
                    build_changelog_entry=None,
                    max_iterations=args.max_build_iterations,
                    subpath=subpath,
                    extra_repositories=extra_repositories,
                )

            try:
                if args.recursive:
                    vcs_directory = os.path.join(args.output_directory, 'vcs')
                    os.makedirs(vcs_directory, exist_ok=True)
                    apt_directory = os.path.join(args.output_directory, 'apt')
                    os.makedirs(apt_directory, exist_ok=True)

                    def main_build():
                        return do_build(
                            wt, subpath, apt_repo.directory,
                            extra_repositories=apt_repo.sources_lines())
                    with SimpleTrustedAptRepo(apt_directory) as apt_repo:
                        (changes_names, cl_entry) = iterate_with_build_fixers(
                            [DebianizeFixer(
                                vcs_directory, apt_repo,
                                diligence=args.diligence,
                                trust=args.trust,
                                check=args.check,
                                net_access=not args.disable_net_access,
                                force_new_directory=args.force_new_directory,
                                team=args.team, verbose=args.verbose,
                                force_subprocess=args.force_subprocess,
                                upstream_version_kind=(
                                    args.upstream_version_kind),
                                debian_revision=args.debian_revision,
                                schroot=args.schroot,
                                unshare=args.unshare, use_inotify=use_inotify,
                                consult_external_directory=(
                                    args.consult_external_directory),
                                create_dist=create_dist,
                                compat_release=compat_release,
                                do_build=do_build)],
                            main_build)
                else:
                    (changes_names, cl_entry) = do_build(
                        wt, subpath, args.output_directory)
            except DetailedDebianBuildFailure as e:
                if e.phase is None:
                    phase = 'unknown phase'
                elif len(e.phase) == 1:
                    phase = e.phase[0]
                else:
                    phase = f'{e.phase[0]} ({e.phase[1]})'
                logging.fatal('Error during %s: %s', phase, e.error)
                return 1
            except UnidentifiedDebianBuildError as e:
                if e.phase is None:
                    phase = 'unknown phase'
                elif len(e.phase) == 1:
                    phase = e.phase[0]
                else:
                    phase = f'{e.phase[0]} ({e.phase[1]})'
                logging.fatal('Error during %s: %s', phase, e.description)
                return 1
            except DebianizedPackageRequirementMismatch as e:
                report_fatal(
                    'package-requirements-mismatch',
                    'Debianized package (binary packages: %r), version %s '
                    'did not satisfy requirement %r. '
                    'Wrong repository (%s)?' % (
                        [binary['Package'] for binary in e.control.binaries],
                        e.version, e.requirement, e.upstream_branch))
                return 1
            logging.info('Built %r.', changes_names)
            if args.install:
                subprocess.check_call(
                    ["debi"] + [
                        os.path.join(args.output_directory, cn)
                        for cn in changes_names])

    if debianize_result.vcs_url:
        target_branch_url = vcs_git_url_to_bzr_url(
            debianize_result.vcs_url)
    else:
        target_branch_url = None

    if os.environ.get("SVP_API") == "1":
        with open(os.environ['SVP_RESULT'], "w") as f:
            json.dump({
                "description": "Debianized package",
                "target-branch-url": target_branch_url,
                "versions": versions_dict(),
                "context": {
                    "wnpp_bugs": debianize_result.wnpp_bugs,
                }
            }, f)

    return 0


if __name__ == "__main__":
    sys.exit(main())
