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

import atexit
import logging
import shutil
import sys
import tempfile

import breezy
import distro_info
from breezy.branch import Branch
from breezy.errors import (  # noqa: E402
    DependencyNotPresent,
    NotBranchError,
)
from breezy.workingtree import WorkingTree
from breezy.workspace import WorkspaceDirty

from debian.changelog import ChangelogCreateError, get_maintainer

breezy.initialize()  # type: ignore
import breezy.bzr  # noqa: E402
import breezy.git  # noqa: E402

from . import (  # noqa: E402
    DEFAULT_MINIMUM_CERTAINTY,
    DescriptionMissing,
    NotDebianPackage,
    _lintian_brush_rs,
    control_files_in_root,
    get_committer,
    run_lintian_fixers,
    select_fixers,
    version_string,
)
from .config import Config  # noqa: E402
from .svp import (  # noqa: E402
    report_fatal,
    report_success_debian,
    svp_enabled,
    load_resume
)

calculate_value = _lintian_brush_rs.calculate_value
LINTIAN_BRUSH_TAG_DEFAULT_VALUE = (
    _lintian_brush_rs.LINTIAN_BRUSH_TAG_DEFAULT_VALUE)
DEFAULT_VALUE_LINTIAN_BRUSH = _lintian_brush_rs.DEFAULT_VALUE_LINTIAN_BRUSH
DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY = (
    _lintian_brush_rs.DEFAULT_VALUE_LINTIAN_BRUSH_ADDON_ONLY)
DEFAULT_ADDON_FIXERS = _lintian_brush_rs.DEFAULT_ADDON_FIXERS
LINTIAN_BRUSH_TAG_VALUES = _lintian_brush_rs.LINTIAN_BRUSH_TAG_VALUES


def versions_dict() -> dict[str, str]:
    import debmutate

    import debian
    return {
        "lintian-brush": version_string,
        "breezy": breezy.version_string,  # type: ignore
        "debmutate": debmutate.version_string,
        "debian": debian.__version__,
    }


def main(fixers, directory, include, dry_run, identity, modern,
         update_changelog, allow_reformatting,
         minimum_certainty, compat_release, yolo,
         uncertain, verbose, trust, opinionated,
         diligence, disable_inotify, exclude,
         disable_net_access, diff) -> int | None:
    logging.basicConfig(level=logging.INFO, format='%(message)s')

    try:
        if dry_run:
            branch, subpath = Branch.open_containing(directory)
            td = tempfile.mkdtemp()
            atexit.register(shutil.rmtree, td)
            # TODO(jelmer): Make a slimmer copy
            to_dir = branch.controldir.sprout(
                td,
                None,
                create_tree_if_local=True,
                source_branch=branch,
                stacked=branch._format.supports_stacking(),
            )
            wt = to_dir.open_workingtree()
        else:
            wt, subpath = WorkingTree.open_containing(directory)
    except NotBranchError:
        logging.error(
            "No version control directory found (e.g. a .git directory).")
        return 1
    except DependencyNotPresent as e:
        logging.error(
            "Unable to open tree at %s: missing package %s",
            directory,
            e.library,  # type: ignore
        )
        return 1
    if identity:
        print("Committer identity: %s" % get_committer(wt))
        print("Changelog identity: {} <{}>".format(*get_maintainer()))
        return 0
    since_revid = wt.last_revision()
    if include or exclude:
        try:
            fixers = select_fixers(
                fixers, names=(include or None), exclude=exclude)
        except KeyError as e:
            logging.error("Unknown fixer specified: %s", e.args[0])
            return 1
    debian_info = distro_info.DebianDistroInfo()
    if modern:
        if compat_release:
            logging.error(
                "--compat-release and --modern are incompatible.")
            return 1
        compat_release = debian_info.devel()
    else:
        compat_release = compat_release
    try:
        cfg = Config.from_workingtree(wt, subpath)
    except FileNotFoundError:
        pass
    else:
        if minimum_certainty is None:
            minimum_certainty = cfg.minimum_certainty()
        if compat_release is None:
            compat_release = cfg.compat_release()
        if allow_reformatting is None:
            allow_reformatting = cfg.allow_reformatting()
        if update_changelog is None:
            update_changelog = cfg.update_changelog()
    if minimum_certainty is None:
        if uncertain or yolo:
            minimum_certainty = "possible"
        else:
            minimum_certainty = DEFAULT_MINIMUM_CERTAINTY
    if compat_release is None:
        compat_release = debian_info.stable()
    if allow_reformatting is None:
        allow_reformatting = False
    with wt.lock_write():
        if control_files_in_root(wt, subpath):
            report_fatal(
                versions_dict(),
                "control-files-in-root",
                "control files live in root rather than debian/ "
                "(LarstIQ mode)",
            )

        try:
            overall_result = run_lintian_fixers(
                wt,
                fixers,
                update_changelog=update_changelog,
                compat_release=compat_release,
                verbose=verbose,
                minimum_certainty=minimum_certainty,
                trust_package=trust,
                allow_reformatting=allow_reformatting,
                use_inotify=(False if disable_inotify else None),
                subpath=subpath,
                net_access=not disable_net_access,
                opinionated=opinionated,
                diligence=diligence,
            )
        except NotDebianPackage:
            report_fatal(
                versions_dict(),
                "not-debian-package", "Not a Debian package")
            return 1
        except WorkspaceDirty:
            logging.error(
                "%s: Please commit pending changes and "
                "remove unknown files first.", wt.basedir)
            if verbose:
                from breezy.status import show_tree_status

                show_tree_status(wt)
            return 1
        except ChangelogCreateError as e:
            report_fatal(
                versions_dict(),
                "changelog-create-error",
                "Error creating changelog entry: %s" % e
            )
            return 1
        except DescriptionMissing as e:
            report_fatal(
                versions_dict(),
                "fixer-description-missing",
                "Fixer %s made changes but did not provide description." %
                e.fixer)
            return 1

    if overall_result.overridden_lintian_issues:
        if len(overall_result.overridden_lintian_issues) == 1:
            logging.info(
                "%d change skipped because of lintian overrides.",
                len(overall_result.overridden_lintian_issues))
        else:
            logging.info(
                "%d changes skipped because of lintian overrides.",
                len(overall_result.overridden_lintian_issues))
    if overall_result.success:
        all_tags = set()
        for result, _summary in overall_result.success:
            all_tags.update(result.fixed_lintian_tags)
        if all_tags:
            logging.info("Lintian tags fixed: %r", all_tags)
        else:
            logging.info(
                "Some changes were made, "
                "but there are no affected lintian tags."
            )
        min_certainty = overall_result.minimum_success_certainty()
        if min_certainty != "certain":
            logging.info(
                "Some changes were made with lower certainty (%s); "
                "please double check the changes.",
                min_certainty,
            )
    else:
        logging.info("No changes made.")
    if overall_result.failed_fixers and not verbose:
        logging.info(
            "Some fixer scripts failed to run: %r. "
            "Run with --verbose for details.",
            set(overall_result.failed_fixers.keys()),
        )
    if overall_result.formatting_unpreservable and not verbose:
        logging.info(
            "Some fixer scripts were unable to preserve formatting: %r. "
            "Run with --allow-reformatting to reformat %r.",
            set(overall_result.formatting_unpreservable),
            set(overall_result.formatting_unpreservable.values()),
        )
    if diff:
        from breezy.diff import show_diff_trees

        show_diff_trees(
            wt.branch.repository.revision_tree(since_revid), wt,
            sys.stdout.buffer
        )
    if svp_enabled():
        applied = []
        base = load_resume()
        if base:
            applied.extend(base['applied'])
        for result, summary in overall_result.success:
            applied.append(
                {
                    "summary": summary,
                    "description": result.description,
                    "fixed_lintian_tags": result.fixed_lintian_tags,
                    "fixed_lintian_issues": [
                        issue.json()
                        for issue in result.fixed_lintian_issues],
                    "revision_id": result.revision_id.decode("utf-8"),
                    "certainty": result.certainty,
                }
            )
        all_fixed_lintian_tags = set()
        for entry in applied:
            all_fixed_lintian_tags.update(entry['fixed_lintian_tags'])
        failed = {
            name: str(e)
            for (name, e) in overall_result.failed_fixers.items()}
        report_success_debian(
            versions_dict(),
            value=calculate_value(list(all_fixed_lintian_tags)),
            context={
                'applied': applied,
                'failed': failed,
            }, changelog=tuple(overall_result.changelog_behaviour)
            if overall_result.changelog_behaviour else None)
    return None
