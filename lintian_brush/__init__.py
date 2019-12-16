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

from debian.changelog import Changelog
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
import warnings

from breezy import ui

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.clean_tree import (
    iter_deletables,
    )
from breezy.commit import NullCommitReporter
from breezy.errors import NoSuchFile
from breezy.rename_map import RenameMap
from breezy.trace import note, warning
from breezy.transform import revert

from debian.deb822 import Deb822


from .reformatting import FormattingUnpreservable


__version__ = (0, 48)
version_string = '.'.join(map(str, __version__))
SUPPORTED_CERTAINTIES = ['certain', 'confident', 'likely', 'possible', None]
DEFAULT_MINIMUM_CERTAINTY = 'certain'
USER_AGENT = 'lintian-brush/' + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3


class NoChanges(Exception):
    """Script didn't make any changes."""


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
                 certainty=None):
        self.description = description
        self.fixed_lintian_tags = fixed_lintian_tags
        self.certainty = certainty

    def __repr__(self):
        return "%s(%r, fixed_lintian_tags=%r, certainty=%r)" % (
                self.__class__.__name__,
                self.description, self.fixed_lintian_tags, self.certainty)

    def __eq__(self, other):
        if type(other) != type(self):
            return False
        return ((self.description == other.description) and
                (self.fixed_lintian_tags == other.fixed_lintian_tags) and
                (self.certainty == other.certainty))


class Fixer(object):
    """A Fixer script.

    The `lintian_tags` attribute contains the name of the lintian tags this
    fixer addresses.
    """

    def __init__(self, name, lintian_tags=None):
        self.name = name
        self.lintian_tags = lintian_tags or []

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty=None, trust_package=False,
            allow_reformatting=False, net_access=True, opinionated=False):
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
        Returns:
          A FixerResult object
        """
        raise NotImplementedError(self.run)


def parse_script_fixer_output(text):
    """Parse the output from a script fixer."""
    lines = []
    fixed_tags = []
    certainty = None
    for line in text.splitlines():
        # TODO(jelmer): Do this in a slighly less hackish manner
        try:
            (key, value) = line.split(':', 1)
        except ValueError:
            lines.append(line)
        else:
            if key == 'Fixed-Lintian-Tags':
                fixed_tags = value.strip().split(',')
            elif key == 'Certainty':
                certainty = value.strip()
            else:
                lines.append(line)
    if certainty not in SUPPORTED_CERTAINTIES:
        raise UnsupportedCertainty(certainty)
    return FixerResult('\n'.join(lines), fixed_tags, certainty)


def determine_env(package, current_version, compat_release, minimum_certainty,
                  trust_package, allow_reformatting, net_access, opinionated):
    env = dict(os.environ.items())
    env['PACKAGE'] = package
    env['CURRENT_VERSION'] = str(current_version)
    env['COMPAT_RELEASE'] = compat_release
    env['MINIMUM_CERTAINTY'] = minimum_certainty
    env['TRUST_PACKAGE'] = 'true' if trust_package else 'false'
    env['REFORMATTING'] = ('allow' if allow_reformatting else 'disallow')
    env['NET_ACCESS'] = ('allow' if net_access else 'disallow')
    env['OPINIONATED'] = ('yes' if opinionated else 'no')
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
            net_access=True, opinionated=False):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated)
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

        if retcode == 2:
            raise NoChanges()
        if retcode != 0:
            raise FixerScriptFailed(self.script_path, retcode, err)

        return parse_script_fixer_output(description)


class ScriptFixer(Fixer):
    """A fixer that is implemented as a shell/python/etc script."""

    def __init__(self, name, lintian_tags, script_path):
        super(ScriptFixer, self).__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return "<ScriptFixer(%r)>" % self.name

    def run(self, basedir, package, current_version, compat_release,
            minimum_certainty=DEFAULT_MINIMUM_CERTAINTY,
            trust_package=False, allow_reformatting=False,
            net_access=True, opinionated=False):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated)
        with tempfile.SpooledTemporaryFile() as stderr:
            p = subprocess.Popen(self.script_path, cwd=basedir,
                                 stdout=subprocess.PIPE, stderr=stderr,
                                 env=env)
            (description, err) = p.communicate("")
            if p.returncode == 2:
                raise NoChanges()
            if p.returncode != 0:
                stderr.seek(0)
                raise FixerScriptFailed(
                        self.script_path, p.returncode,
                        stderr.read().decode('utf-8', 'replace'))
        return parse_script_fixer_output(description.decode('utf-8'))


def find_fixers_dir():
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


def read_desc_file(path):
    """Read a description file.

    Args:
      path: Path to read from.
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
            if script_path.endswith('.py'):
                yield PythonScriptFixer(name, tags, script_path)
            else:
                yield ScriptFixer(name, tags, script_path)


def select_fixers(fixers, names, exclude=None):
    """Select fixers by name, from a list.

    Args:
      fixers: List of Fixer objects
      names: List of names to select
      exclude: List of names to exclude
    Raises:
      KeyError: if one of the names did not exist
    """
    names = set(names)
    if exclude:
        for name in exclude:
            if name not in names:
                raise KeyError(name)
            names.remove(name)
    available = set([f.name for f in fixers])
    missing = names - available
    if missing:
        raise KeyError(missing.pop())
    # Preserve order
    return [f for f in fixers if f.name in names]


def available_lintian_fixers(fixers_dir=None):
    """Return a list of available lintian fixers.

    Args:
      fixers_dir: Fixers directory to browse
    Returns:
      Iterator over Fixer objects
    """
    if fixers_dir is None:
        fixers_dir = find_fixers_dir()
    for n in os.listdir(fixers_dir):
        if not n.endswith(".desc"):
            continue
        for fixer in read_desc_file(os.path.join(fixers_dir, n)):
            yield fixer


def increment_version(v):
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


def delete_items(deletables, dry_run=False):
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


def get_committer(tree):
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
                user = cs.get(("user", ), "name")
            except KeyError:
                user = None
            else:
                user = user.decode('utf-8')
        if email is None:
            try:
                email = cs.get(("user", ), "email")
            except KeyError:
                email = None
            else:
                email = email.decode('utf-8')
        if user and email:
            return user + " <" + email + ">"
        from breezy.config import GlobalStack
        return GlobalStack().get('email')
    else:
        config = tree.branch.get_config_stack()
        return config.get('email')


def only_changes_last_changelog_block(tree, changelog_path):
    """Check whether the only change in a tree is to the last changelog entry.

    Args:
      tree: Tree to analyze
      changelog_path: Path to the changelog file
    Returns:
      boolean
    """
    basis_tree = tree.basis_tree()
    with tree.lock_read(), basis_tree.lock_read():
        changes = tree.iter_changes(basis_tree)
        try:
            first_change = next(changes)
        except StopIteration:
            return False
        try:
            next(changes)
        except StopIteration:
            pass
        else:
            return False
        try:
            change_paths = first_change.path
        except AttributeError:  # brz < 3.1
            change_paths = first_change[1]
        if change_paths != (changelog_path, changelog_path):
            return False
        new_cl = Changelog(tree.get_file_text(changelog_path))
        old_cl = Changelog(basis_tree.get_file_text(changelog_path))
        if old_cl.distributions != "UNRELEASED":
            return False
        del new_cl._blocks[0]
        del old_cl._blocks[0]
        return str(new_cl) == str(old_cl)


def reset_tree(local_tree, dirty_tracker=None, subpath='.'):
    """Reset a tree back to its basis tree.

    This will leave ignored and detritus files alone.

    Args:
      local_tree: tree to work on
      dirty_tracker: Optional dirty tracker
    """
    if dirty_tracker and not dirty_tracker.is_dirty():
        return
    revert(local_tree, local_tree.branch.basis_tree(),
           [subpath] if subpath != '.' else None)
    deletables = list(iter_deletables(
        local_tree, unknown=True, ignored=False, detritus=False))
    delete_items(deletables)


def certainty_sufficient(actual_certainty, minimum_certainty):
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


def check_clean_tree(local_tree):
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


def add_changelog_entry(tree, path, summary):
    """Add a changelog entry.

    Args:
      tree: Tree to edit
      path: Path to the changelog file
      summary: Entry to add
    """
    p = subprocess.Popen(
        ["dch", "--no-auto-nmu", summary],
        cwd=tree.abspath(os.path.dirname(os.path.dirname(path))),
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE)
    (stdout, stderr) = p.communicate(b"\n")
    if stdout:
        raise ValueError(stdout)
    if stderr:
        end = b'dch: Did you see that warning?  Press RETURN to continue...\n'
        if stderr.endswith(end):
            stderr = stderr[:-len(end)]
        for line in stderr.splitlines():
            warning('%s', line)


def gbp_conf_has_dch_section(tree, path=''):
    try:
        gbp_conf_path = os.path.join(path, 'debian/gbp.conf')
        gbp_conf_text = tree.get_file_text(gbp_conf_path)
    except NoSuchFile:
        return False
    try:
        import configparser
    except ImportError:
        return None
    else:
        parser = configparser.ConfigParser(defaults={}, strict=False)
        parser.read_string(
            gbp_conf_text.decode('utf-8', errors='replace'), gbp_conf_path)
        return parser.has_section('dch')


def all_sha_prefixed(cl):
    sha_prefixed = 0
    for change in cl.changes():
        if not change.startswith('  * '):
            continue
        if re.match(r'  \* \[[0-9a-f]{7}\] ', change):
            sha_prefixed += 1
        else:
            return False
    return (sha_prefixed > 0)


def guess_update_changelog(tree, path='', cl=None):
    """Guess whether the changelog should be updated.

    Args:
      tree: Tree to edit
      path: Path to packaging in tree
    """
    if gbp_conf_has_dch_section(tree, path):
        note('Assuming changelog does not need to be updated, '
             'since there is a [dch] section in gbp.conf.')
        return False

    # TODO(jelmes): Do something more clever here, perhaps looking at history
    # of the changelog file?
    changelog_path = os.path.join(path, 'debian/changelog')
    if cl is None:
        try:
            with tree.get_file(changelog_path) as f:
                cl = Changelog(f, max_blocks=1)
        except NoSuchFile:
            cl = None

    if cl and all_sha_prefixed(cl[0]):
        return False

    return True


def run_lintian_fixer(local_tree, fixer, committer=None,
                      update_changelog=None, compat_release=None,
                      minimum_certainty=None, trust_package=False,
                      allow_reformatting=False, dirty_tracker=None,
                      subpath='.', net_access=True, opinionated=None):
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
            opinionated=opinionated)
    except BaseException:
        reset_tree(local_tree, dirty_tracker, subpath)
        raise
    if not certainty_sufficient(result.certainty, minimum_certainty):
        reset_tree(local_tree, dirty_tracker, subpath)
        raise NoChanges("Certainty of script's changes not high enough")
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
            raise NoChanges("Script didn't make any changes")
    else:
        local_tree.smart_add([local_tree.abspath(subpath)])
        specific_files = [subpath] if subpath != '.' else None

    basis_tree = local_tree.basis_tree()
    if local_tree.supports_setting_file_ids():
        RenameMap.guess_renames(
            basis_tree, local_tree, dry_run=False)

    if len(local_tree.get_parent_ids()) <= 1:
        changes = local_tree.iter_changes(
            basis_tree, specific_files=specific_files)
        try:
            next(changes)
        except StopIteration:
            raise NoChanges("Script didn't make any changes")

    if not result.description:
        raise DescriptionMissing()

    summary = result.description.splitlines()[0]
    if update_changelog is None:
        update_changelog = guess_update_changelog(local_tree, subpath, cl)

    if update_changelog and only_changes_last_changelog_block(
            local_tree, changelog_path):
        # If the script only changed the last entry in the changelog,
        # don't update the changelog
        update_changelog = False

    if update_changelog:
        add_changelog_entry(local_tree, changelog_path, summary)
        if specific_files:
            specific_files.append(changelog_path)

    description = result.description
    for tag in result.fixed_lintian_tags:
        description += "\n"
        description += "\n"
        description += "Fixes lintian: %s\n" % tag
        description += ("See https://lintian.debian.org/tags/%s.html "
                        "for more details.\n") % tag

    if committer is None:
        committer = get_committer(local_tree)

    local_tree.commit(description, allow_pointless=False,
                      reporter=NullCommitReporter(),
                      committer=committer,
                      specific_files=specific_files)
    # TODO(jelmer): Run sbuild & verify lintian warning is gone?
    return result, summary


class ManyResult(object):

    def __init__(self):
        self.success = []
        self.failed_fixers = {}
        self.formatting_unpreservable = {}

    def __tuple__(self):
        return (self.success, self.failed_fixers)

    def __iter__(self):
        return iter(self.__tuple__())


def get_dirty_tracker(local_tree, subpath='', use_inotify=None):
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


def run_lintian_fixers(local_tree, fixers, update_changelog=True,
                       verbose=False, committer=None,
                       compat_release=None, minimum_certainty=None,
                       trust_package=False, allow_reformatting=False,
                       use_inotify=None, subpath='.', net_access=True,
                       opinionated=None):
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
                        opinionated=opinionated)
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


def certainty_to_confidence(certainty):
    if certainty in ('unknown', None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def confidence_to_certainty(confidence):
    if confidence is None:
        return 'unknown'
    try:
        return SUPPORTED_CERTAINTIES[confidence]
    except IndexError:
        raise ValueError(confidence)


def min_certainty(certainties):
    return confidence_to_certainty(
        max([certainty_to_confidence(c)
            for c in certainties] + [0]))
