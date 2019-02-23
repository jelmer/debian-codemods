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
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import warnings

from breezy import ui

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.clean_tree import (
    iter_deletables,
    )
from breezy.commit import NullCommitReporter
from breezy.rename_map import RenameMap
from breezy.trace import note
from breezy.transform import revert

from debian.deb822 import Deb822


__version__ = (0, 16)
version_string = '.'.join(map(str, __version__))
SUPPORTED_CERTAINTIES = ['certain', 'possible', None]


class NoChanges(Exception):
    """Script didn't make any changes."""


class FixerFailed(Exception):
    """Base class for fixer script failures."""


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
                self.errors.decode(errors='replace')))


class DescriptionMissing(Exception):
    """The fixer script did not provide a description on stdout."""


class NotDebianPackage(Exception):
    """The specified directory does not contain a Debian package."""

    def __init__(self, tree):
        super(NotDebianPackage, self).__init__(tree.basedir)


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

    def __init__(self, name, lintian_tags):
        self.name = name
        self.lintian_tags = lintian_tags

    def run(self, basedir, current_version, compat):
        """Apply this fixer script.

        Args:
          basedir: Directory in which to run
          current_version: The version of the package that is being created or
            updated
          compat: Compatibility level (a Debian release name)
        Returns:
          A FixerResult object
        """
        raise NotImplementedError(self.run)


class ScriptFixer(Fixer):
    """A fixer that is implemented as a shell/python/etc script."""

    def __init__(self, name, lintian_tags, script_path):
        super(ScriptFixer, self).__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return "<ScriptFixer(%r)>" % self.name

    def run(self, basedir, current_version, compat_release):
        env = dict(os.environ.items())
        env['CURRENT_VERSION'] = str(current_version)
        env['COMPAT_RELEASE'] = compat_release
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
                        stderr.read())
        description = description.decode('utf-8')
        lines = []
        fixed_tags = []
        certainty = None
        for line in description.splitlines():
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
            yield ScriptFixer(name, tags, script_path)


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


def only_changes_last_changelog_block(tree):
    """Check whether the only change in a tree is to the last changelog entry.

    Args:
      tree: Tree to analyze
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
        if first_change[1] != ('debian/changelog', 'debian/changelog'):
            return False
        new_cl = Changelog(tree.get_file_text('debian/changelog'))
        old_cl = Changelog(basis_tree.get_file_text('debian/changelog'))
        if old_cl.distributions != "UNRELEASED":
            return False
        del new_cl._blocks[0]
        del old_cl._blocks[0]
        return str(new_cl) == str(old_cl)


def run_lintian_fixer(local_tree, fixer, committer=None,
                      update_changelog=None, compat_release=None):
    """Run a lintian fixer on a tree.

    Args:
      local_tree: WorkingTree object
      fixer: Fixer object to apply
      committer: Optional committer (name and email)
      update_changelog: Whether to add a new entry to the changelog
      compat_release: Minimum release that the package should be usable on
        (e.g. 'stable' or 'unstable')
    Returns:
      tuple with set of FixerResult, summary of the changes
    """
    # Just check there are no changes to begin with
    if local_tree.has_changes():
        raise PendingChanges(local_tree)
    if list(local_tree.unknowns()):
        raise PendingChanges(local_tree)
    if not local_tree.has_filename('debian/changelog'):
        raise NotDebianPackage(local_tree)
    with local_tree.get_file('debian/changelog') as f:
        cl = Changelog(f, max_blocks=1)
    if cl.distributions == 'UNRELEASED':
        current_version = cl.version
    else:
        current_version = cl.version
        increment_version(current_version)
    if compat_release is None:
        compat_release = 'sid'
    try:
        result = fixer.run(local_tree.basedir, current_version=current_version,
                           compat_release=compat_release)
    except BaseException:
        revert(local_tree, local_tree.branch.basis_tree(), None)
        deletables = list(iter_deletables(
            local_tree, unknown=True, ignored=False, detritus=False))
        delete_items(deletables)
        raise
    local_tree.smart_add([local_tree.basedir])
    if local_tree.supports_setting_file_ids():
        RenameMap.guess_renames(
            local_tree.basis_tree(), local_tree, dry_run=False)

    if not local_tree.has_changes():
        raise NoChanges("Script didn't make any changes")

    if not result.description:
        raise DescriptionMissing()

    summary = result.description.splitlines()[0]
    if update_changelog is None:
        # Default to true. Perhaps do something more clever.
        update_changelog = True

    if update_changelog and only_changes_last_changelog_block(local_tree):
        # If the script only changed the last entry in the changelog,
        # don't update the changelog
        update_changelog = False

    if update_changelog:
        subprocess.check_call(
            ["dch", "--no-auto-nmu", summary], cwd=local_tree.basedir)

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
                      committer=committer)
    # TODO(jelmer): Run sbuild & verify lintian warning is gone?
    return result, summary


def run_lintian_fixers(local_tree, fixers, update_changelog=True,
                       verbose=False, committer=None,
                       compat_release=None):
    """Run a set of lintian fixers on a tree.

    Args:
      local_tree: WorkingTree object
      fixers: A set of Fixer objects
      update_changelog: Whether to add an entry to the changelog
      verbose: Whether to be verbose
      committer: Optional committer (name and email)
      compat_release: Minimum release that the package should be usable on
        (e.g. 'sid' or 'stretch')
    Returns:
      Tuple with two lists:
        list of tuples with (lintian-tag, certainty, description) of fixers
        that ran list of script names for fixers that failed to run
    """
    failed_fixers = []
    fixers = list(fixers)
    ret = []
    with ui.ui_factory.nested_progress_bar() as pb:
        for i, fixer in enumerate(fixers):
            pb.update('Running fixer %r on %s' % (fixer, local_tree.basedir),
                      i, len(fixers))
            start = time.time()
            try:
                result, summary = run_lintian_fixer(
                        local_tree, fixer, update_changelog=update_changelog,
                        committer=committer, compat_release=compat_release)
            except FixerFailed as e:
                failed_fixers.append(fixer.name)
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
                ret.append((result, summary))
    return ret, failed_fixers
