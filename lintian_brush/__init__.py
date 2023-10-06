#!/usr/bin/python
# Copyright (C) 2018 Jelmer Vernooij
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

"""Automatically fix lintian issues."""

import logging
import os
import sys
import time
from contextlib import ExitStack
from datetime import datetime
from typing import (
    Iterable,
    List,
    Optional,
    Tuple,
)

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.tree import Tree
from breezy.workingtree import WorkingTree
from breezy.workspace import check_clean_tree
from debmutate.reformatting import FormattingUnpreservable

from . import _lintian_brush_rs

__version__ = (0, 151)
version_string = ".".join(map(str, __version__))
SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]
DEFAULT_MINIMUM_CERTAINTY = "certain"
USER_AGENT = "lintian-brush/" + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3
logger = logging.getLogger(__name__)


DEFAULT_ADDON_FIXERS = _lintian_brush_rs.DEFAULT_ADDON_FIXERS
LintianIssue = _lintian_brush_rs.LintianIssue
FixerResult = _lintian_brush_rs.FixerResult
UnsupportedCertainty = _lintian_brush_rs.UnsupportedCertainty
read_desc_file = _lintian_brush_rs.read_desc_file
only_changes_last_changelog_block = (
    _lintian_brush_rs.only_changes_last_changelog_block)


class NoChanges(Exception):
    """Script didn't make any changes."""

    def __init__(self, fixer, comment=None, overridden_lintian_issues=None):
        super().__init__(fixer, comment)
        self.fixer = fixer
        self.overridden_lintian_issues = overridden_lintian_issues or []


class NotCertainEnough(NoChanges):
    """Script made changes but with too low certainty."""

    def __init__(self, fixer, certainty, minimum_certainty,
                 overridden_lintian_issues=None):
        super().__init__(
            fixer, overridden_lintian_issues=overridden_lintian_issues)
        self.certainty = certainty
        self.minimum_certainty = minimum_certainty


class FixerFailed(Exception):
    """Base class for fixer script failures."""

    def __eq__(self, other):
        if not isinstance(other, self.__class__):
            return False
        return self.args == other.args


class FixerScriptFailed(FixerFailed):
    """Script failed to run."""

    def __init__(self, path, returncode, errors):
        self.path = path
        self.returncode = returncode
        self.errors = errors

    def __str__(self):
        return "Script %s failed with exit code: %d\n%s\n" % (
            self.path,
            self.returncode,
            self.errors,
        )

    def __eq__(self, other):
        if not isinstance(other, self.__class__):
            return False
        return (
            self.path == other.path
            and self.returncode == other.returncode
            and self.errors == other.errors
        )


class DescriptionMissing(Exception):
    """The fixer script did not provide a description on stdout."""

    def __init__(self, fixer):
        super().__init__(fixer)
        self.fixer = fixer


class NotDebianPackage(Exception):
    """The specified directory does not contain a Debian package."""

    def __init__(self, abspath):
        super().__init__(abspath)


parse_script_fixer_output = _lintian_brush_rs.parse_script_fixer_output
determine_env = _lintian_brush_rs.determine_env
Fixer = _lintian_brush_rs.Fixer
ScriptFixer = _lintian_brush_rs.ScriptFixer
PythonScriptFixer = _lintian_brush_rs.PythonScriptFixer


def open_binary(name):
    return open(data_file_path(name), 'rb')  # noqa: SIM115


def data_file_path(name, check=os.path.exists):
    # There's probably a more Pythonic way of doing this, but
    # I haven't bothered finding out what it is yet..
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), "..", name))
    if check(path):
        return path

    import pkg_resources

    path = pkg_resources.resource_filename(
        __name__, f"lintian-brush/{name}")
    if check(path):
        return path

    # Urgh.
    for b in ['/usr/share/lintian-brush',
              '/usr/local/share/lintian-brush',
              os.path.join(sys.prefix, 'share/lintian-brush')]:
        path = os.path.join(b, name)
        if check(path):
            return path
    raise RuntimeError("unable to find data path: %s" % name)


find_fixers_dir = _lintian_brush_rs.find_fixers_dir


def select_fixers(
    fixers: List[Fixer], *, names: Optional[List[str]] = None,
    exclude: Optional[Iterable[str]] = None
) -> List[Fixer]:
    """Select fixers by name, from a list.

    Args:
      fixers: List of Fixer objects
      names: Set of names to select
      exclude: Set of names to exclude
    Raises:
      KeyError: if one of the names did not exist
    """
    select_set = set(names) if names is not None else None
    exclude_set = set(exclude) if exclude is not None else None
    ret = []
    for f in fixers:
        if select_set is not None:
            if f.name not in select_set:
                continue
            select_set.remove(f.name)
        if exclude_set and f.name in exclude_set:
            exclude_set.remove(f.name)
            continue
        ret.append(f)
    if select_set:
        raise KeyError(select_set.pop())
    if exclude_set:
        raise KeyError(exclude_set.pop())
    return ret


def available_lintian_fixers(fixers_dir=None, force_subprocess=False):
    if fixers_dir is None:
        fixers_dir = find_fixers_dir()
    return _lintian_brush_rs.available_lintian_fixers(
        fixers_dir, force_subprocess)


increment_version = _lintian_brush_rs.increment_version


def get_committer(tree: WorkingTree) -> str:
    """Get the committer string for a tree.

    Args:
      tree: A Tree object
    Returns:
      A committer string
    """
    # TODO(jelmer): Perhaps this logic should be in Breezy?
    if hasattr(tree.branch.repository, "_git"):
        cs = tree.branch.repository._git.get_config_stack()
        user = os.environ.get("GIT_COMMITTER_NAME")
        email = os.environ.get("GIT_COMMITTER_EMAIL")
        if user is None:
            try:
                user = cs.get(("user",), "name").decode("utf-8")
            except KeyError:
                user = None
        if email is None:
            try:
                email = cs.get(("user",), "email").decode("utf-8")
            except KeyError:
                email = None
        if user and email:
            return user + " <" + email + ">"
        from breezy.config import GlobalStack

        return GlobalStack().get("email")
    else:
        config = tree.branch.get_config_stack()
        return config.get("email")


certainty_sufficient = _lintian_brush_rs.certainty_sufficient


_changelog_policy_noted = False


def _note_changelog_policy(policy, msg):
    global _changelog_policy_noted
    if not _changelog_policy_noted:
        if policy:
            extra = "Specify --no-update-changelog to override."
        else:
            extra = "Specify --update-changelog to override."
        logging.info("%s %s", msg, extra)
    _changelog_policy_noted = True


class FailedPatchManipulation(Exception):
    def __init__(self, tree, patches_directory, reason):
        super().__init__(
            tree, patches_directory, reason)


def _upstream_changes_to_patch(
    local_tree: WorkingTree,
    basis_tree: Tree,
    dirty_tracker,
    subpath: str,
    patch_name: str,
    patch_description: str,
    timestamp: Optional[datetime] = None,
) -> Tuple[str, List[str]]:
    from .patches import (
        PatchSyntax,
        move_upstream_changes_to_patch,
        read_quilt_patches,
        tree_patches_directory,
    )

    # TODO(jelmer): Apply all patches before generating a diff.

    patches_directory = tree_patches_directory(local_tree, subpath)
    try:
        quilt_patches = list(read_quilt_patches(local_tree, patches_directory))
    except PatchSyntax as e:
        raise FailedPatchManipulation(
            local_tree, patches_directory,
            "Unable to parse some patches: %s" % e) from e
    if len(quilt_patches) > 0:
        raise FailedPatchManipulation(
            local_tree,
            patches_directory,
            "Creating patch on top of existing upstream "
            "patches not supported.",
        )

    logging.debug("Moving upstream changes to patch %s", patch_name)
    try:
        specific_files, patch_name = move_upstream_changes_to_patch(
            local_tree,
            basis_tree,
            subpath,
            patch_name,
            patch_description,
            dirty_tracker,
            timestamp=timestamp,
        )
    except FileExistsError as e:
        raise FailedPatchManipulation(
            local_tree, patches_directory,
            "patch path %s already exists\n" % e.args[0]
        ) from e

    return patch_name, specific_files


run_lintian_fixer = _lintian_brush_rs.run_lintian_fixer


class ManyResult:
    def __init__(self):
        self.success = []
        self.failed_fixers = {}
        self.formatting_unpreservable = {}
        self.overridden_lintian_issues = []
        self.changelog_behaviour = None

    def minimum_success_certainty(self) -> str:
        """Return the minimum certainty of any successfully made change."""
        return min_certainty(
            [
                r.certainty
                for r, unused_summary in self.success
                if r.certainty is not None
            ]
        )


def determine_update_changelog(local_tree, debian_path):
    from .detect_gbp_dch import (
        ChangelogBehaviour,
        guess_update_changelog,
    )

    changelog_path = os.path.join(debian_path, 'changelog')

    if not local_tree.has_filename(changelog_path):
        # If there's no changelog, then there's nothing to update!
        return False

    behaviour = guess_update_changelog(local_tree, debian_path)
    if behaviour:
        _note_changelog_policy(
            behaviour.update_changelog, behaviour.explanation)
    else:
        # If we can't make an educated guess, assume yes.
        behaviour = ChangelogBehaviour(
            True, "Assuming changelog should be updated")

    return behaviour


def run_lintian_fixers(  # noqa: C901
    local_tree: WorkingTree,
    fixers: List[Fixer],
    update_changelog: bool = True,
    verbose: bool = False,
    committer: Optional[str] = None,
    compat_release: Optional[str] = None,
    minimum_certainty: Optional[str] = None,
    trust_package: bool = False,
    allow_reformatting: bool = False,
    use_inotify: Optional[bool] = None,
    subpath: str = "",
    net_access: bool = True,
    opinionated: Optional[bool] = None,
    diligence: int = 0,
):
    """Run a set of lintian fixers on a tree.

    Args:
      local_tree: WorkingTree object
      fixers: A set of Fixer objects
      update_changelog: Whether to add an entry to the changelog
      verbose: Whether to be verbose
      committer: Optional committer (name and email)
      compat_release: Minimum release that the package should be usable on
        (e.g. 'sid' or 'stretch')
      minimum_certainty: How certain the fixer should be
        about its changes.
      trust_package: Whether to run code from the package if necessary
      allow_reformatting: Whether to allow reformatting of changed files
      use_inotify: Use inotify to watch changes (significantly improves
        performance). Defaults to None (automatic)
      subpath: Subpath in the tree in which the package lives
      net_access: Whether to allow network access
      opinionated: Whether to be opinionated
      diligence: Level of diligence
    Returns:
      Tuple with two lists:
        1. list of tuples with (lintian-tag, certainty, description) of fixers
           that ran
        2. dictionary mapping fixer names for fixers that failed to run to the
           error that occurred
    """
    from tqdm import trange
    basis_tree = local_tree.basis_tree()
    check_clean_tree(local_tree, basis_tree=basis_tree, subpath=subpath)
    fixers = list(fixers)

    # If we don't know whether to update the changelog, then find out *once*
    if update_changelog is None:
        changelog_behaviour = None

        def update_changelog():
            nonlocal update_changelog, changelog_behaviour
            changelog_behaviour = determine_update_changelog(
                local_tree, os.path.join(subpath, "debian"))
            return changelog_behaviour.update_changelog
    else:
        changelog_behaviour = None

    ret = ManyResult()
    with ExitStack() as es:
        t = es.enter_context(
            trange(len(fixers), leave=False, disable=None))  # type: ignore

        dirty_tracker = get_dirty_tracker(
            local_tree, subpath=subpath, use_inotify=use_inotify
        )
        if dirty_tracker:
            from breezy.dirty_tracker import TooManyOpenFiles
            try:
                es.enter_context(dirty_tracker)
            except TooManyOpenFiles:
                logging.warning(
                    'Too many open files for inotify, not using it.')
                dirty_tracker = None

        for fixer in fixers:
            t.set_description("Running fixer %s" % fixer)
            t.update()
            start = time.time()
            if dirty_tracker:
                dirty_tracker.mark_clean()
            try:
                result, summary = run_lintian_fixer(
                    local_tree,
                    fixer,
                    update_changelog=update_changelog,
                    committer=committer,
                    compat_release=compat_release,
                    minimum_certainty=minimum_certainty,
                    trust_package=trust_package,
                    allow_reformatting=allow_reformatting,
                    dirty_tracker=dirty_tracker,
                    subpath=subpath,
                    net_access=net_access,
                    opinionated=opinionated,
                    diligence=diligence,
                    basis_tree=basis_tree,
                )
            except FormattingUnpreservable as e:
                ret.formatting_unpreservable[fixer.name] = e.path
                if verbose:
                    logging.info(
                        "Fixer %r was unable to preserve "
                        "formatting of %s.", fixer.name,
                        e.path)
            except FixerFailed as e:
                ret.failed_fixers[fixer.name] = e
                if verbose:
                    logging.info("Fixer %r failed to run.", fixer.name)
                    sys.stderr.write(str(e))
            except MemoryError as e:
                ret.failed_fixers[fixer.name] = e
                if verbose:
                    logging.info(
                        "Run out of memory while running fixer %r.",
                        fixer.name)
            except NotCertainEnough as e:
                if verbose:
                    logging.info(
                        "Fixer %r made changes but not high enough "
                        "certainty (was %r, needed %r). (took: %.2fs)",
                        fixer.name,
                        e.certainty,
                        e.minimum_certainty,
                        time.time() - start,
                    )
            except FailedPatchManipulation as e:
                if verbose:
                    logging.info(
                        "Unable to manipulate upstream patches: %s",
                        e.args[2])
                ret.failed_fixers[fixer.name] = e
            except NoChanges as e:
                if verbose:
                    logging.info(
                        "Fixer %r made no changes. (took: %.2fs)",
                        fixer.name,
                        time.time() - start,
                    )
                ret.overridden_lintian_issues.extend(
                    e.overridden_lintian_issues)
            else:
                if verbose:
                    logging.info(
                        "Fixer %r made changes. (took %.2fs)",
                        fixer.name,
                        time.time() - start,
                    )
                ret.success.append((result, summary))
                basis_tree = local_tree.basis_tree()
    if changelog_behaviour:
        ret.changelog_behaviour = changelog_behaviour
    return ret


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    if certainty in ("unknown", None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


min_certainty = _lintian_brush_rs.min_certainty
control_files_in_root = _lintian_brush_rs.control_files_in_root
is_debcargo_package = _lintian_brush_rs.is_debcargo_package
control_file_present = _lintian_brush_rs.control_file_present
