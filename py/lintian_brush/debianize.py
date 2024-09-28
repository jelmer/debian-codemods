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
    "NoBuildToolsFound",
    "debianize",
]

import contextlib
import errno
import json
import logging
import os
import shutil
import subprocess
from dataclasses import dataclass, field
from functools import partial
from tempfile import TemporaryDirectory
from typing import Any, Callable, Dict, List, Optional, Tuple, cast
from urllib.parse import urlparse

from breezy import osutils
from breezy.branch import Branch
from breezy.commit import NullCommitReporter, PointlessCommit
from breezy.controldir import ControlDir
from breezy.errors import (
    AlreadyBranchError,
    NoSuchRevision,
    NotBranchError,
)
from breezy.plugins.debian import default_orig_dir
from breezy.plugins.debian.directory import vcs_git_url_to_bzr_url
from breezy.plugins.debian.import_dsc import UpstreamAlreadyImported
from breezy.plugins.debian.merge_upstream import (
    do_import,
    get_existing_imported_upstream_revids,
    get_tarballs,
)
from breezy.plugins.debian.upstream import (
    PackageVersionNotPresent,
    UpstreamSource,
)
from breezy.plugins.debian.upstream.branch import (
    DistCommandFailed,
    UpstreamBranchSource,
    run_dist_command,
    upstream_version_add_revision,
)
from breezy.plugins.debian.upstream.pristinetar import (
    BasePristineTarSource,
    get_pristine_tar_source,
)
from breezy.revision import NULL_REVISION, RevisionID
from breezy.transport import FileExists
from breezy.tree import Tree
from breezy.workingtree import WorkingTree
from breezy.workspace import (
    WorkspaceDirty,
    check_clean_tree,
    reset_tree,
)
from buildlog_consultant.common import (
    SetuptoolScmVersionIssue,
    VcsControlDirectoryNeeded,
)
from debmutate.control import (
    ControlEditor,
    ensure_exact_version,
    ensure_relation,
    ensure_some_version,
)
from debmutate.vcs import unsplit_vcs_url
from debmutate.versions import (
    debianize_upstream_version,
)
from ognibuild import DetailedFailure, UnidentifiedError
from ognibuild.buildlog import problem_to_upstream_requirement
from ognibuild.buildsystem import (
    BuildSystem,
    NoBuildToolsFound,
    get_buildsystem,
)
from ognibuild.debian.apt import AptManager
from ognibuild.debian.fix_build import (
    DetailedDebianBuildFailure,
    UnidentifiedDebianBuildError,
    build_incrementally,
    default_fixers,
)
from ognibuild.debian.upstream_deps import get_project_wide_deps
from ognibuild.dist import (  # noqa: F401
    DistNoTarball,
)
from ognibuild.dist import (
    create_dist as ogni_create_dist,
)
from ognibuild.fix_build import BuildFixer, iterate_with_build_fixers
from ognibuild.requirements import (
    Requirement,
)
from ognibuild.resolver.apt import AptRequirement
from ognibuild.session import Session, SessionSetupFailure
from ognibuild.session.plain import PlainSession
from ognibuild.session.schroot import SchrootSession
from ognibuild.upstream import (
    find_apt_upstream,
    find_upstream,
    go_base_name,
    load_crate_info,
)
from ognibuild.vcs import dupe_vcs_tree
from upstream_ontologist.debian import (
    upstream_name_to_debian_source_name as source_name_from_upstream_name,
)
from upstream_ontologist.debian import (
    upstream_version_to_debian_upstream_version as debian_upstream_version,
)
from upstream_ontologist.debian import (
    valid_debian_package_name,
)
from upstream_ontologist.guess import (
    UpstreamDatum,
    guess_upstream_info,
    summarize_upstream_metadata,
)

from debian.changelog import Version, get_maintainer
from debian.deb822 import PkgRelation

from . import (
    _debianize_rs,
    available_lintian_fixers,
    get_committer,
    run_lintian_fixers,
)
from .debbugs import find_archived_wnpp_bugs, find_wnpp_bugs
from .debhelper import (
    maximum_debhelper_compat_version,
)
from .debhelper import (
    write_rules_template as write_debhelper_rules_template,
)
from .publish import (
    NoVcsLocation,
    update_official_vcs,
)
from .standards_version import latest_standards_version
from .svp import svp_enabled

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


MINIMUM_CERTAINTY = "possible"  # For now..


class DebianDirectoryExists(Exception):
    """A Debian Directory already exists."""

    def __init__(self, path):
        self.path = path

def default_create_dist(
    session, tree, package, version, target_dir, subpath=""
):
    try:
        with session:
            try:
                return ogni_create_dist(
                    session,
                    tree,
                    target_dir,
                    include_controldir=False,
                    subdir=(package or "package"),
                    version=version,
                    subpath=subpath,
                )
            except DetailedFailure as e:
                if isinstance(
                    e.error,
                    (VcsControlDirectoryNeeded, SetuptoolScmVersionIssue),
                ):
                    return ogni_create_dist(
                        session,
                        tree,
                        target_dir,
                        include_controldir=True,
                        subdir=(package or "package"),
                        version=version,
                        subpath=subpath,
                    )
                else:
                    raise
    except NoBuildToolsFound:
        logging.info("No build tools found, falling back to simple export.")
        return None
    except NotImplementedError:
        logging.info(
            "Build system does not support dist, falling back " "to export."
        )
        return None
    except SessionSetupFailure as exc:
        raise DistCommandFailed(str(exc), "session-setup-failure") from exc
    except DistNoTarball as e:
        logging.info("Build system did not create a tarball: %s", e)
        return None
    except DetailedFailure as exc:
        raise DistCommandFailed(str(exc), exc.error) from exc
    except UnidentifiedError as exc:
        raise DistCommandFailed(str(exc)) from exc


def import_upstream_version_from_dist(
    wt,
    subpath,
    upstream_source,
    source_name,
    upstream_version,
    session: Session,
) -> Tuple[
    Dict[Optional[str], Tuple[RevisionID, str]],
    Dict[Optional[str], RevisionID],
    str,
]:
    orig_dir = os.path.abspath(default_orig_dir)

    tag_names = {}
    with TemporaryDirectory() as target_dir:
        locations = upstream_source.fetch_tarballs(
            source_name, upstream_version, target_dir, components=[None]
        )
        if source_name is None:
            source_name = os.path.basename(locations[0]).split("_")[0]
        try:
            tarball_filenames = get_tarballs(
                orig_dir, wt, source_name, upstream_version, locations
            )
        except FileExists as e:
            logging.warning(
                "Tarball %s exists, reusing existing file.", e.path
            )
            tarball_filenames = [os.path.join(orig_dir, e.path)]
        upstream_revisions = upstream_source.version_as_revisions(
            source_name, upstream_version
        )
        files_excluded = None
        try:
            imported_revids = do_import(
                wt,
                subpath,
                tarball_filenames,
                source_name,
                upstream_version,
                current_version=None,
                upstream_branch=upstream_source.upstream_branch,
                upstream_revisions=upstream_revisions,
                merge_type=None,
                files_excluded=files_excluded,
            )
        except UpstreamAlreadyImported as e:
            logging.warning("Upstream release %s already imported.", e.version)  # type: ignore
            imported_revids = get_existing_imported_upstream_revids(
                upstream_source, source_name, upstream_version
            )
        pristine_revids = {}
        for (
            component,
            tag_name,
            revid,
            _pristine_tar_imported,
            subpath,
        ) in imported_revids:
            pristine_revids[component] = (revid, subpath)
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


@dataclass
class DebianizeResult:
    """Debianize result."""

    upstream_branch_name: Optional[str] = None
    tag_names: Dict[Optional[str], RevisionID] = field(default_factory=dict)
    upstream_version: Optional[str] = None
    wnpp_bugs: List[Tuple[int, str]] = field(default_factory=list)
    vcs_url: Optional[str] = None


def import_upstream_dist(
    pristine_tar_source: BasePristineTarSource,
    wt: WorkingTree,
    upstream_source: UpstreamSource,
    subpath: str,
    source_name: str,
    upstream_version: str,
    session: Session,
) -> Tuple[
    Tuple[RevisionID, str], Optional[str], Dict[Optional[str], RevisionID]
]:
    if pristine_tar_source.has_version(
        source_name, upstream_version, try_hard=False
    ):
        logging.warning(
            "Upstream version %s/%s already imported.",
            source_name,
            upstream_version,
        )
        pristine_revids = pristine_tar_source.version_as_revisions(
            source_name, upstream_version
        )
        upstream_branch_name = None
        tag_names: Dict[Optional[str], RevisionID] = {}
    else:
        (
            pristine_revids,
            tag_names,
            upstream_branch_name,
        ) = import_upstream_version_from_dist(
            wt,
            subpath,
            upstream_source,
            source_name,
            upstream_version,
            session=session,
        )

    assert isinstance(pristine_revids[None], tuple) and len(
        pristine_revids[None]
    ), repr(pristine_revids[None])
    return pristine_revids[None], upstream_branch_name, tag_names


def get_upstream_version(
    upstream_source,
    metadata: Dict[str, Any],
    local_dir=None,
    upstream_subpath: Optional[str] = None,
    upstream_version: Optional[str] = None,
) -> Tuple[str, str]:
    # TODO(jelmer): if upstream_subpath != "", perhaps ignore info from
    # upstream_source?
    if upstream_version is None:
        (
            upstream_version,
            mangled_upstream_version,
        ) = upstream_source.get_latest_version(metadata.get("Name"), None)
    else:
        mangled_upstream_version = debianize_upstream_version(upstream_version)
    if upstream_version is None:
        raise NoUpstreamReleases(upstream_source, metadata.get("Name"))

    upstream_revision = upstream_source.version_as_revision(
        metadata.get("Name"), mangled_upstream_version
    )

    if upstream_version is None and "Version" in metadata:
        # They haven't done any releases yet. Assume we're ahead of
        # the next announced release?
        next_upstream_version = debian_upstream_version(metadata["Version"])
        upstream_version = upstream_version_add_revision(
            upstream_source.upstream_branch,
            next_upstream_version,
            upstream_revision,
            "~",
        )
    if upstream_version is None:
        upstream_version = upstream_version_add_revision(
            upstream_source.upstream_branch, "0", upstream_revision, "+"
        )
        logging.warning(
            "Unable to determine upstream version, using %s.", upstream_version
        )
    return upstream_version, mangled_upstream_version


def debianize(  # noqa: C901
    wt: WorkingTree,
    subpath: str,
    *,
    upstream_branch: Optional[Branch],
    upstream_subpath: Optional[str],
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
    metadata: Optional[Dict[str, Any]] = None,
):
    if committer is None:
        committer = get_committer(wt)

    debian_path = osutils.pathjoin(subpath, "debian")
    if (
        wt.has_filename(debian_path)
        and list(os.listdir(wt.abspath(debian_path)))
        and not force_new_directory
    ):
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
                metadata_items,
                p,
                net_access=net_access,
                consult_external_directory=consult_external_directory,
                check=check,
            )
        )

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
                logging.info("Using schroot %s", schroot)
                session = SchrootSession(schroot)
            elif unshare:
                logging.info("Using tarball %s for unshare", unshare)
                from ognibuild.session.unshare import (
                    UnshareSession,  # type: ignore
                )

                session = UnshareSession.from_tarball(unshare)
            else:
                session = PlainSession()

            if upstream_branch:
                upstream_source = UpstreamBranchSource.from_branch(
                    upstream_branch,
                    version_kind=upstream_version_kind,
                    local_dir=wt.controldir,
                    create_dist=(
                        create_dist or partial(default_create_dist, session)
                    ),
                )
            else:
                upstream_source = None

            if upstream_version is not None:
                mangled_upstream_version = debianize_upstream_version(
                    upstream_version
                )
            else:
                (
                    upstream_version,
                    mangled_upstream_version,
                ) = get_upstream_version(
                    upstream_source,
                    metadata,
                    local_dir=wt.controldir,
                    upstream_subpath=upstream_subpath,
                    upstream_version=upstream_version,
                )

            result.upstream_version = upstream_version

            source_name = generic_get_source_name(wt, subpath, metadata)

            def kickstart_from_dist(wt, subpath):
                logging.info(
                    "Kickstarting from dist tarball. "
                    "Using upstream version %s",
                    mangled_upstream_version,
                )

                pristine_tar_source = get_pristine_tar_source(wt, wt.branch)
                (
                    (upstream_dist_revid, upstream_dist_subpath),
                    result.upstream_branch_name,
                    result.tag_names,
                ) = import_upstream_dist(
                    pristine_tar_source,
                    wt,
                    upstream_source,
                    subpath,
                    source_name,
                    mangled_upstream_version,
                    session,
                )

                if wt.branch.last_revision() != upstream_dist_revid:
                    wt.pull(
                        upstream_source.upstream_branch,
                        overwrite=True,
                        stop_revision=upstream_dist_revid,
                    )

                    # Gather metadata items again now that we're at the correct
                    # revision
                    import_metadata_from_path(
                        wt.abspath(upstream_dist_subpath)
                    )

                if wt.has_filename(debian_path) and force_new_directory:
                    shutil.rmtree(wt.abspath(debian_path))
                    wt.mkdir(wt.abspath(debian_path))
                    with contextlib.suppress(PointlessCommit):
                        wt.commit(
                            "Remove old debian directory",
                            specific_files=[debian_path],
                            reporter=NullCommitReporter(),
                        )

                wt.mkdir(os.path.join(debian_path, "source"))
                wt.add(os.path.join(debian_path, "source"))
                wt.put_file_bytes_non_atomic(
                    os.path.join(debian_path, "source", "format"),
                    b"3.0 (quilt)\n",
                )

            if upstream_source:
                try:
                    (
                        upstream_vcs_tree,
                        upstream_vcs_subpath,
                    ) = upstream_source.revision_tree(
                        source_name, mangled_upstream_version
                    )
                except (PackageVersionNotPresent, NoSuchRevision):
                    logging.warning(
                        "Unable to find upstream version %s/%s "
                        "in upstream source %r. Unable to extract metadata.",
                        source_name,
                        mangled_upstream_version,
                        upstream_source,
                    )
                    exported_upstream_tree_path = None
                else:
                    assert upstream_vcs_subpath == ""
                    # TODO(jelmer): Don't export, just access from memory.
                    exported_upstream_tree_path = es.enter_context(
                        TemporaryDirectory()
                    )
                    assert isinstance(upstream_subpath, str)
                    dupe_vcs_tree(
                        upstream_vcs_tree, exported_upstream_tree_path
                    )
                    exported_upstream_tree_subpath = os.path.join(
                        exported_upstream_tree_path, upstream_subpath
                    )
                    if not os.path.isdir(exported_upstream_tree_subpath):
                        raise Exception(
                            f"subdirectory {upstream_subpath} does not "
                            f"exist in upstream version {upstream_version}"
                        )
                    import_metadata_from_path(exported_upstream_tree_subpath)

            if (
                buildsystem_name is None
                and exported_upstream_tree_path is not None
            ):
                buildsystem_subpath, buildsystem = get_buildsystem(
                    os.path.join(exported_upstream_tree_path, subpath)
                )
                if buildsystem:
                    buildsystem_name = buildsystem.name
            else:
                buildsystem = None
                buildsystem_subpath = ""

            if buildsystem_name:
                try:
                    process = PROCESSORS[buildsystem_name]
                except KeyError:
                    logging.warning(
                        "No support in debianize for build system %s, "
                        "falling back to default.",
                        buildsystem_name,
                    )
                    process = process_default
            else:
                process = process_default

            logging.info("Creating core packaging using %s", process.__name__)

            os.chdir(wt.abspath(subpath))

            control = process(
                es,
                session,
                wt,
                subpath,
                debian_path,
                upstream_version,
                metadata,
                compat_release,
                buildsystem,
                buildsystem_subpath,
                kickstart_from_dist,
            )

            source = control.source

            if team:
                control.source["Maintainer"] = team
                uploader = "{} <{}>".format(*get_maintainer())
                if uploader != team:
                    control.source["Uploaders"] = uploader

            if not valid_debian_package_name(source["Source"]):
                raise SourcePackageNameInvalid(source["Source"])

            if net_access:
                wnpp_bugs = find_wnpp_bugs_harder(
                    source["Source"], metadata.get("Name")
                )
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

            if (
                requirement
                and requirement.family == "apt"
                and not cast(AptRequirement, requirement).satisfied_by(
                    control.binaries, version
                )
            ):
                logging.warning(
                    "Debianized package (binary packages: %r), version %s "
                    "did not satisfy requirement %r. ",
                    [binary["Package"] for binary in control.binaries],
                    version,
                    requirement,
                )
                if upstream_branch:
                    logging.info("Wrong repository (%s)?", upstream_branch)
                raise DebianizedPackageRequirementMismatch(
                    requirement, control, version, upstream_branch
                )

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
            force_subprocess=force_subprocess
        )

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
            result.vcs_url = unsplit_vcs_url(
                *update_official_vcs(wt, subpath=subpath, committer=committer)
            )
        except NoVcsLocation:
            logging.debug(
                "No public VCS location specified and unable to guess it "
                "based on maintainer e-mail."
            )
        except FileNotFoundError:
            logging.info(
                "No control file or debcargo.toml file, "
                "not setting vcs information."
            )

    return result
