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

from debian.changelog import Changelog, Version
import errno
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import traceback
from typing import Optional, List, Sequence, Iterator, Iterable
import warnings

from breezy import ui

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.clean_tree import (
    iter_deletables,
    )
from breezy.commit import NullCommitReporter
from breezy.errors import NoSuchFile
from breezy.osutils import is_inside
from breezy.rename_map import RenameMap
from breezy.trace import note
from breezy.transform import revert
from breezy.workingtree import WorkingTree

from debian.deb822 import Deb822


from debmutate.reformatting import FormattingUnpreservable


__version__ = (0, 75)
version_string = '.'.join(map(str, __version__))
SUPPORTED_CERTAINTIES = ['certain', 'confident', 'likely', 'possible', None]
DEFAULT_MINIMUM_CERTAINTY = 'certain'
USER_AGENT = 'lintian-brush/' + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3


class NoChanges(Exception):
    """Script didn't make any changes."""

    def __init__(self, fixer, comment=None):
        super(NoChanges, self).__init__(fixer, comment)
        self.fixer = fixer


class NotCertainEnough(NoChanges):
    """Script made changes but with too low certainty."""

    def __init__(self, fixer, certainty, minimum_certainty):
        super(NotCertainEnough, self).__init__(fixer)
        self.certainty = certainty
        self.minimum_certainty = minimum_certainty


class FixerFailed(Exception):
    """Base class for fixer script failures."""

    def __eq__(self, other):
        if not isinstance(other, self.__class__):
            return False
        return self.args == other.args


class UnsupportedCertainty(Exception):
    """Unsupported certainty."""


class FixerScriptFailed(FixerFailed):
    """Script failed to run."""

    def __init__(self, path, returncode, errors):
        self.path = path
        self.returncode = returncode
        self.errors = errors

    def __str__(self):
        return ("Script %s failed with exit code: %d\n%s\n" % (
                self.path, self.returncode,
                self.errors))

    def __eq__(self, other):
        if not isinstance(other, self.__class__):
            return False
        return (
            self.path == other.path and
            self.returncode == other.returncode and
            self.errors == other.errors)


class DescriptionMissing(Exception):
    """The fixer script did not provide a description on stdout."""

    def __init__(self, fixer):
        super(DescriptionMissing, self).__init__(fixer)
        self.fixer = fixer


class NotDebianPackage(Exception):
    """The specified directory does not contain a Debian package."""

    def __init__(self, tree, path):
        super(NotDebianPackage, self).__init__(tree.abspath(path))


class PendingChanges(Exception):
    """The directory has pending changes."""

    def __init__(self, tree):
        super(PendingChanges, self).__init__(tree.basedir)


class FixerResult(object):
    """Result of a fixer run."""

    def __init__(self, description, fixed_lintian_tags=[],
                 certainty=None, patch_name=None,
                 revision_id=None):
        self.description = description
        self.fixed_lintian_tags = fixed_lintian_tags
        self.certainty = certainty
        self.patch_name = patch_name
        self.revision_id = revision_id

    def __repr__(self):
        return ("%s(%r, fixed_lintian_tags=%r, certainty=%r, patch_name=%r, "
                "revision_id=%r)") % (
                self.__class__.__name__,
                self.description, self.fixed_lintian_tags, self.certainty,
                self.patch_name, self.revision_id)

    def __eq__(self, other):
        if type(other) != type(self):
            return False
        return ((self.description == other.description) and
                (self.fixed_lintian_tags == other.fixed_lintian_tags) and
                (self.certainty == other.certainty) and
                (self.patch_name == other.patch_name) and
                (self.revision_id == other.revision_id))


class Fixer(object):
    """A Fixer script.

    The `lintian_tags` attribute contains the name of the lintian tags this
    fixer addresses.
    """

    def __init__(self, name: str, lintian_tags: List[str] = None):
        self.name = name
        self.lintian_tags = lintian_tags or []

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty=None, trust_package=False,
            allow_reformatting=False, net_access=True, opinionated=False,
            diligence=0):
        """Apply this fixer script.

        Args:
          basedir: Directory in which to run
          package: Name of the source package
          current_version: The version of the package that is being created or
            updated
          compat_release: Compatibility level (a Debian release name)
          trust_package: Whether to run code from the package
          allow_reformatting: Allow reformatting of files that are being
            changed
          opinionated: Whether to be opinionated
          diligence: Level of diligence
        Returns:
          A FixerResult object
        """
        raise NotImplementedError(self.run)


def parse_script_fixer_output(text):
    """Parse the output from a script fixer."""
    lines = []
    fixed_tags = []
    certainty = None
    patch_name = None
    for line in text.splitlines():
        # TODO(jelmer): Do this in a slighly less hackish manner
        try:
            (key, value) = line.split(':', 1)
        except ValueError:
            lines.append(line)
        else:
            if key == 'Fixed-Lintian-Tags':
                fixed_tags = [tag.strip() for tag in value.strip().split(',')]
            elif key == 'Certainty':
                certainty = value.strip()
            elif key == 'Patch-Name':
                patch_name = value.strip()
            else:
                lines.append(line)
    if certainty not in SUPPORTED_CERTAINTIES:
        raise UnsupportedCertainty(certainty)
    return FixerResult('\n'.join(lines), fixed_tags, certainty, patch_name)


def determine_env(package, current_version, compat_release, minimum_certainty,
                  trust_package, allow_reformatting, net_access, opinionated,
                  diligence):
    env = dict(os.environ.items())
    env['PACKAGE'] = package
    env['CURRENT_VERSION'] = str(current_version)
    env['COMPAT_RELEASE'] = compat_release
    env['MINIMUM_CERTAINTY'] = minimum_certainty
    env['TRUST_PACKAGE'] = 'true' if trust_package else 'false'
    env['REFORMATTING'] = ('allow' if allow_reformatting else 'disallow')
    env['NET_ACCESS'] = ('allow' if net_access else 'disallow')
    env['OPINIONATED'] = ('yes' if opinionated else 'no')
    env['DILIGENCE'] = str(diligence)
    return env


class PythonScriptFixer(Fixer):
    """A fixer that is implemented as a python script.

    This gets used just for Python scripts, and significantly speeds
    things up because it prevents starting a new Python interpreter
    for every fixer.
    """

    def __init__(self, name, lintian_tags, script_path):
        super(PythonScriptFixer, self).__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return "<%s(%r)>" % (self.__class__.__name__, self.name)

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty=DEFAULT_MINIMUM_CERTAINTY,
            trust_package=False, allow_reformatting=False,
            net_access=True, opinionated=False, diligence=0):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated,
            diligence=diligence)
        try:
            old_env = os.environ
            old_stderr = sys.stderr
            old_stdout = sys.stdout
            sys.stderr = io.StringIO()
            sys.stdout = io.StringIO()
            os.environ = env
            old_cwd = os.getcwd()
            try:
                os.chdir(basedir)
                global_vars = {
                    "__file__": self.script_path,
                    "__name__": "__main__",
                    }
                with open(self.script_path, 'r') as f:
                    code = compile(f.read(), self.script_path, 'exec')
                    exec(code, global_vars)
            except FormattingUnpreservable:
                raise
            except SystemExit as e:
                retcode = e.code
            except BaseException as e:
                traceback.print_exception(
                    type(e), e, e.__traceback__, file=sys.stderr)
                raise FixerScriptFailed(
                    self.script_path, 1,
                    sys.stderr.getvalue())
            else:
                retcode = 0
            description = sys.stdout.getvalue()
            err = sys.stderr.getvalue()
        finally:
            os.environ = old_env
            sys.stderr = old_stderr
            sys.stdout = old_stdout
            os.chdir(old_cwd)
            from . import fixer
            fixer.reset()

        if retcode == 2:
            raise NoChanges(self)
        if retcode != 0:
            raise FixerScriptFailed(self.script_path, retcode, err)

        return parse_script_fixer_output(description)


class ScriptFixer(Fixer):
    """A fixer that is implemented as a shell/python/etc script."""

    def __init__(self, name: str, lintian_tags: List[str], script_path: str):
        super(ScriptFixer, self).__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return "<ScriptFixer(%r)>" % self.name

    def run(self,
            basedir: str,
            package: str,
            current_version: Version,
            compat_release: str,
            minimum_certainty: str = DEFAULT_MINIMUM_CERTAINTY,
            trust_package: bool = False,
            allow_reformatting: bool = False,
            net_access: bool = True,
            opinionated: bool = False,
            diligence: int = 0):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated,
            diligence=diligence)
        with tempfile.SpooledTemporaryFile() as stderr:
            try:
                p = subprocess.Popen(self.script_path, cwd=basedir,
                                     stdout=subprocess.PIPE, stderr=stderr,
                                     env=env)
            except OSError as e:
                if e.errno == errno.ENOMEM:
                    raise MemoryError
                raise
            (description, err) = p.communicate(b"")
            if p.returncode == 2:
                raise NoChanges(self)
            if p.returncode != 0:
                stderr.seek(0)
                raise FixerScriptFailed(
                        self.script_path, p.returncode,
                        stderr.read().decode('utf-8', 'replace'))
        return parse_script_fixer_output(description.decode('utf-8'))


def find_fixers_dir() -> str:
    """Find the local directory with lintian fixer scripts."""
    local_dir = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'fixers'))
    if os.path.isdir(local_dir):
        return local_dir
    import pkg_resources
    resource_dir = pkg_resources.resource_filename(
        __name__, 'lintian-brush/fixers')
    if os.path.isdir(resource_dir):
        return resource_dir
    # Urgh.
    return '/usr/share/lintian-brush/fixers'


def read_desc_file(
        path: str, force_subprocess: bool = False) -> Iterator[Fixer]:
    """Read a description file.

    Args:
      path: Path to read from.
      force_subprocess: Force running as subprocess
    Yields:
      Fixer objects
    """
    dirname = os.path.dirname(path)
    with open(path, 'r') as f:
        for paragraph in Deb822.iter_paragraphs(f):
            name = os.path.splitext(paragraph['Fix-Script'])[0]
            script_path = os.path.join(dirname, paragraph['Fix-Script'])
            if 'Lintian-Tags' in paragraph:
                tags = [tag.strip()
                        for tag in paragraph['Lintian-Tags'].split(',')]
            else:
                tags = []
            if script_path.endswith('.py') and not force_subprocess:
                yield PythonScriptFixer(name, tags, script_path)
            else:
                yield ScriptFixer(name, tags, script_path)


def select_fixers(fixers: List[Fixer],
                  names: List[str],
                  exclude: Optional[Iterable[str]] = None) -> List[Fixer]:
    """Select fixers by name, from a list.

    Args:
      fixers: List of Fixer objects
      names: Set of names to select
      exclude: Set of names to exclude
    Raises:
      KeyError: if one of the names did not exist
    """
    names_set = set(names)
    if exclude:
        for name in exclude:
            if name not in names_set:
                raise KeyError(name)
            names_set.remove(name)
    available = set([f.name for f in fixers])
    missing = names_set - available
    if missing:
        raise KeyError(missing.pop())
    # Preserve order
    return [f for f in fixers if f.name in names_set]


def available_lintian_fixers(
        fixers_dir: Optional[str] = None,
        force_subprocess: bool = False) -> Iterator[Fixer]:
    """Return a list of available lintian fixers.

    Args:
      fixers_dir: Fixers directory to browse
      force_subprocess: Force running fixers from subprocess
    Returns:
      Iterator over Fixer objects
    """
    if fixers_dir is None:
        fixers_dir = find_fixers_dir()
    for n in os.listdir(fixers_dir):
        if not n.endswith(".desc"):
            continue
        for fixer in read_desc_file(
                os.path.join(fixers_dir, n),
                force_subprocess=force_subprocess):
            yield fixer


def increment_version(v: Version) -> None:
    """Increment a version number.

    For native packages, increment the main version number.
    For other packages, increment the debian revision.

    Args:
        v: Version to increment (modified in place)
    """
    if v.debian_revision is not None:
        v.debian_revision = re.sub(
                '\\d+$', lambda x: str(int(x.group())+1), v.debian_revision)
    else:
        v.upstream_version = re.sub(
                '\\d+$', lambda x: str(int(x.group())+1), v.upstream_version)


def delete_items(deletables, dry_run: bool = False):
    """Delete files in the deletables iterable"""
    def onerror(function, path, excinfo):
        """Show warning for errors seen by rmtree.
        """
        # Handle only permission error while removing files.
        # Other errors are re-raised.
        if function is not os.remove or excinfo[1].errno != errno.EACCES:
            raise
        warnings.warn('unable to remove %s' % path)
    for path, subp in deletables:
        if os.path.isdir(path):
            shutil.rmtree(path, onerror=onerror)
        else:
            try:
                os.unlink(path)
            except OSError as e:
                # We handle only permission error here
                if e.errno != errno.EACCES:
                    raise e
                warnings.warn(
                    'unable to remove "{0}": {1}.'.format(path, e.strerror))


def get_committer(tree: WorkingTree) -> str:
    """Get the committer string for a tree.

    Args:
      tree: A Tree object
    Returns:
      A committer string
    """
    # TODO(jelmer): Perhaps this logic should be in Breezy?
    if getattr(tree.branch.repository, '_git', None):
        cs = tree.branch.repository._git.get_config_stack()
        user = os.environ.get("GIT_COMMITTER_NAME")
        email = os.environ.get("GIT_COMMITTER_EMAIL")
        if user is None:
            try:
                user = cs.get(("user", ), "name").decode('utf-8')
            except KeyError:
                user = None
        if email is None:
            try:
                email = cs.get(("user", ), "email").decode('utf-8')
            except KeyError:
                email = None
        if user and email:
            return user + " <" + email + ">"
        from breezy.config import GlobalStack
        return GlobalStack().get('email')
    else:
        config = tree.branch.get_config_stack()
        return config.get('email')


def only_changes_last_changelog_block(
        tree: WorkingTree,
        changelog_path: str,
        changes) -> bool:
    """Check whether the only change in a tree is to the last changelog entry.

    Args:
      tree: Tree to analyze
      changelog_path: Path to the changelog file
      changes: Changes in the tree
    Returns:
      boolean
    """
    basis_tree = tree.basis_tree()
    with tree.lock_read(), basis_tree.lock_read():
        for change in changes:
            if change.path == ('', ''):
                continue
            if change.path != (changelog_path, changelog_path):
                return False
            break
        else:
            return False
        new_cl = Changelog(tree.get_file_text(changelog_path))
        old_cl = Changelog(basis_tree.get_file_text(changelog_path))
        if old_cl.distributions != "UNRELEASED":
            return False
        del new_cl._blocks[0]
        del old_cl._blocks[0]
        return str(new_cl) == str(old_cl)


def reset_tree(local_tree: WorkingTree, dirty_tracker=None,
               subpath: str = '') -> None:
    """Reset a tree back to its basis tree.

    This will leave ignored and detritus files alone.

    Args:
      local_tree: tree to work on
      dirty_tracker: Optional dirty tracker
      subpath: Subpath to operate on
    """
    if dirty_tracker and not dirty_tracker.is_dirty():
        return
    revert(local_tree, local_tree.branch.basis_tree(),
           [subpath] if subpath not in ('.', '') else None)
    deletables = list(iter_deletables(
        local_tree, unknown=True, ignored=False, detritus=False))
    delete_items(deletables)


def certainty_sufficient(actual_certainty: str,
                         minimum_certainty: Optional[str]) -> bool:
    """Check if the actual certainty is sufficient.

    Args:
      actual_certainty: Actual certainty with which changes were made
      minimum_certainty: Minimum certainty to keep changes
    Returns:
      boolean
    """
    actual_confidence = certainty_to_confidence(actual_certainty)
    if actual_confidence is None:
        # Actual confidence is unknown.
        # TODO(jelmer): Should we really be ignoring this?
        return True
    minimum_confidence = certainty_to_confidence(minimum_certainty)
    if minimum_confidence is None:
        return True
    return actual_confidence <= minimum_confidence


def check_clean_tree(local_tree: WorkingTree) -> None:
    """Check that a tree is clean and has no pending changes or unknown files.

    Args:
      local_tree: The tree to check
    Raises:
      PendingChanges: When there are pending changes
    """
    # Just check there are no changes to begin with
    if local_tree.has_changes():
        raise PendingChanges(local_tree)
    if list(local_tree.unknowns()):
        raise PendingChanges(local_tree)


def has_non_debian_changes(changes):
    for change in changes:
        for path in change.path:
            if path and not is_inside('debian', path):
                return True
    return False


_changelog_policy_noted = False


def _note_changelog_policy(policy, msg):
    global _changelog_policy_noted
    if not _changelog_policy_noted:
        if policy:
            extra = 'Specify --no-update-changelog to override.'
        else:
            extra = 'Specify --update-changelog to override.'
        note('%s %s', msg, extra)
    _changelog_policy_noted = True


def run_lintian_fixer(local_tree: WorkingTree,
                      fixer: Fixer,
                      committer: Optional[str] = None,
                      update_changelog: Optional[bool] = None,
                      compat_release: Optional[str] = None,
                      minimum_certainty: Optional[str] = None,
                      trust_package: bool = False,
                      allow_reformatting: bool = False,
                      dirty_tracker=None,
                      subpath: str = '.',
                      net_access: bool = True,
                      opinionated: Optional[bool] = None,
                      diligence: int = 0):
    """Run a lintian fixer on a tree.

    Args:
      local_tree: WorkingTree object
      fixer: Fixer object to apply
      committer: Optional committer (name and email)
      update_changelog: Whether to add a new entry to the changelog
      compat_release: Minimum release that the package should be usable on
        (e.g. 'stable' or 'unstable')
      minimum_certainty: How certain the fixer should be
        about its changes.
      trust_package: Whether to run code from the package if necessary
      allow_reformatting: Whether to allow reformatting of changed files
      dirty_tracker: Optional object that can be used to tell if the tree
        has been changed.
      subpath: Path in tree to operate on
      net_access: Whether to allow accessing external services
      opinionated: Whether to be opinionated
      diligence: Level of diligence
    Returns:
      tuple with set of FixerResult, summary of the changes
    """
    if subpath == '.':
        changelog_path = 'debian/changelog'
    else:
        changelog_path = os.path.join(subpath, 'debian/changelog')

    try:
        with local_tree.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)
    except NoSuchFile:
        raise NotDebianPackage(local_tree, subpath)
    package = cl.package
    if cl.distributions == 'UNRELEASED':
        current_version = cl.version
    else:
        current_version = cl.version
        increment_version(current_version)
    if compat_release is None:
        compat_release = 'sid'
    if minimum_certainty is None:
        minimum_certainty = DEFAULT_MINIMUM_CERTAINTY
    try:
        result = fixer.run(
            local_tree.abspath(subpath),
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated,
            diligence=diligence)
    except BaseException:
        reset_tree(local_tree, dirty_tracker, subpath)
        raise
    if not certainty_sufficient(result.certainty, minimum_certainty):
        reset_tree(local_tree, dirty_tracker, subpath)
        raise NotCertainEnough(fixer, result.certainty, minimum_certainty)
    specific_files: Optional[List[str]]
    if dirty_tracker:
        relpaths = dirty_tracker.relpaths()
        # Sort paths so that directories get added before the files they
        # contain (on VCSes where it matters)
        local_tree.add(
            [p for p in sorted(relpaths)
             if local_tree.has_filename(p) and not
                local_tree.is_ignored(p)])
        specific_files = [
            p for p in relpaths
            if local_tree.is_versioned(p)]
        if not specific_files:
            raise NoChanges(fixer, "Script didn't make any changes")
    else:
        local_tree.smart_add([local_tree.abspath(subpath)])
        specific_files = [subpath] if subpath != '.' else None

    basis_tree = local_tree.basis_tree()
    if local_tree.supports_setting_file_ids():
        RenameMap.guess_renames(
            basis_tree, local_tree, dry_run=False)

    changes = list(local_tree.iter_changes(
        basis_tree, specific_files=specific_files,
        want_unversioned=False, require_versioned=True))

    if len(local_tree.get_parent_ids()) <= 1 and not changes:
        raise NoChanges(fixer, "Script didn't make any changes")

    if not result.description:
        raise DescriptionMissing(fixer)

    lines = result.description.splitlines()
    summary = lines[0]
    details = lines[1:]

    # If there are upstream changes in a non-native package, perhaps
    # export them to debian/patches
    if has_non_debian_changes(changes) and current_version.debian_revision:
        # TODO(jelmer): Apply all patches before generating a diff.
        reset_tree(local_tree, dirty_tracker, subpath)
        raise NoChanges("Creating upstream patches not supported yet")

        from .patches import move_upstream_changes_to_patch
        try:
            specific_files, patch_name = move_upstream_changes_to_patch(
                local_tree, subpath, result.patch_name, result.description,
                dirty_tracker)
        except FileExistsError as e:
            raise NoChanges('patch path %s already exists\n' % e.args[0])

        summary = 'Add patch %s: %s' % (patch_name, summary)

    if update_changelog is None:
        from .detect_gbp_dch import guess_update_changelog
        dch_guess = guess_update_changelog(local_tree, subpath, cl)
        if dch_guess:
            update_changelog = dch_guess[0]
            _note_changelog_policy(update_changelog, dch_guess[1])
        else:
            # Assume we should update changelog
            update_changelog = True

    if update_changelog and only_changes_last_changelog_block(
            local_tree, changelog_path, changes):
        # If the script only changed the last entry in the changelog,
        # don't update the changelog
        update_changelog = False

    if update_changelog:
        from .changelog import add_changelog_entry
        add_changelog_entry(local_tree, changelog_path, [summary] + details)
        if specific_files:
            specific_files.append(changelog_path)

    description = result.description + "\n"
    description += "\n"
    description += "Changes-By: lintian-brush\n"
    for tag in result.fixed_lintian_tags:
        description += "Fixes: lintian: %s\n" % tag
        description += (
            "See-also: https://lintian.debian.org/tags/%s.html\n" % tag)

    if committer is None:
        committer = get_committer(local_tree)

    revid = local_tree.commit(
        description, allow_pointless=False,
        reporter=NullCommitReporter(),
        committer=committer,
        specific_files=specific_files)
    result.revision_id = revid
    # TODO(jelmer): Run sbuild & verify lintian warning is gone?
    return result, summary


class ManyResult(object):

    def __init__(self):
        self.success = []
        self.failed_fixers = {}
        self.formatting_unpreservable = {}

    def minimum_success_certainty(self) -> str:
        """Return the minimum certainty of any successfully made change."""
        return min_certainty(
            [r.certainty for r, unused_summary in self.success
             if r.certainty is not None])

    def __tuple__(self):
        return (self.success, self.failed_fixers)

    def __iter__(self):
        return iter(self.__tuple__())


def get_dirty_tracker(local_tree: WorkingTree,
                      subpath: str = '',
                      use_inotify: bool = None):
    """Create a dirty tracker object."""
    if use_inotify is True:
        from .dirty_tracker import DirtyTracker
        return DirtyTracker(local_tree, subpath)
    elif use_inotify is False:
        return None
    else:
        try:
            from .dirty_tracker import DirtyTracker
        except ImportError:
            return None
        else:
            return DirtyTracker(local_tree, subpath)


def run_lintian_fixers(local_tree: WorkingTree,
                       fixers: List[Fixer],
                       update_changelog: bool = True,
                       verbose: bool = False,
                       committer: Optional[str] = None,
                       compat_release: Optional[str] = None,
                       minimum_certainty: Optional[str] = None,
                       trust_package: bool = False,
                       allow_reformatting: bool = False,
                       use_inotify: Optional[bool] = None,
                       subpath: str = '.',
                       net_access: bool = True,
                       opinionated: Optional[bool] = None,
                       diligence: int = 0):
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
    check_clean_tree(local_tree)
    fixers = list(fixers)
    dirty_tracker = get_dirty_tracker(
        local_tree, subpath=subpath, use_inotify=use_inotify)
    ret = ManyResult()
    with ui.ui_factory.nested_progress_bar() as pb:
        for i, fixer in enumerate(fixers):
            pb.update('Running fixer %r on %s' %
                      (fixer, local_tree.abspath(subpath)),
                      i, len(fixers))
            start = time.time()
            if dirty_tracker:
                dirty_tracker.mark_clean()
            try:
                result, summary = run_lintian_fixer(
                        local_tree, fixer, update_changelog=update_changelog,
                        committer=committer, compat_release=compat_release,
                        minimum_certainty=minimum_certainty,
                        trust_package=trust_package,
                        allow_reformatting=allow_reformatting,
                        dirty_tracker=dirty_tracker,
                        subpath=subpath, net_access=net_access,
                        opinionated=opinionated,
                        diligence=diligence)
            except FormattingUnpreservable as e:
                ret.formatting_unpreservable[fixer.name] = e.path
                if verbose:
                    note('Fixer %r was unable to preserve formatting of %s.',
                         fixer.name, e.path)
            except FixerFailed as e:
                ret.failed_fixers[fixer.name] = e
                if verbose:
                    note('Fixer %r failed to run.', fixer.name)
                    sys.stderr.write(str(e))
            except MemoryError as e:
                ret.failed_fixers[fixer.name] = e
                if verbose:
                    note('Run out of memory while running fixer %r.',
                         fixer.name)
            except NotCertainEnough as e:
                if verbose:
                    note('Fixer %r made changes but not high enough '
                         'certainty (was %r, needed %r). (took: %.2fs)',
                         fixer.name, e.certainty, e.minimum_certainty,
                         time.time() - start)
            except NoChanges:
                if verbose:
                    note('Fixer %r made no changes. (took: %.2fs)',
                         fixer.name, time.time() - start)
            else:
                if verbose:
                    note('Fixer %r made changes. (took %.2fs)',
                         fixer.name, time.time() - start)
                ret.success.append((result, summary))
    return ret


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    if certainty in ('unknown', None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def confidence_to_certainty(confidence: Optional[int]) -> str:
    if confidence is None:
        return 'unknown'
    try:
        return SUPPORTED_CERTAINTIES[confidence] or 'unknown'
    except IndexError:
        raise ValueError(confidence)


def min_certainty(certainties: Sequence[str]) -> str:
    return confidence_to_certainty(
        max([certainty_to_confidence(c)
            for c in certainties] + [0]))


def load_renamed_tags():
    import json
    path = os.path.abspath(os.path.join(
        os.path.dirname(__file__), '..', 'renamed-tags.json'))
    if not os.path.isfile(path):
        import pkg_resources
        path = pkg_resources.resource_filename(
            __name__, 'lintian-brush/renamed-tags.json')
        if not os.path.isfile(path):
            # Urgh.
            path = '/usr/share/lintian-brush/renamed-tags.json'
    with open(path, 'rb') as f:
        return json.load(f)
