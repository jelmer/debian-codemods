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

import os
import subprocess
import sys

from breezy.clean_tree import (
    iter_deletables,
    delete_items,
    _filter_out_nested_controldirs,
    )
from breezy.rename_map import RenameMap
from breezy.trace import note
from breezy.transform import revert


__version__ = (0, 1)
version_string = '.'.join(map(str, __version__))


class NoChanges(Exception):
    """Script didn't make any changes."""


class ScriptFailed(Exception):
    """Script failed to run."""


class DescriptionMissing(Exception):
    """The fixer script did not provide a description on stdout."""


class FixerResult(object):
    """Result of a fixer run."""

    def __init__(self, description, fixed_lintian_tags=[]):
        self.description = description
        self.fixed_lintian_tags = fixed_lintian_tags


class Fixer(object):
    """A Fixer script.

    The `lintian_tags` attribute contains the name of the lintian tags this
    fixer addresses.
    """

    def __init__(self, lintian_tags):
        self.lintian_tags = lintian_tags

    def run(self, basedir):
        """Apply this fixer script.

        Args:
          basedir: Directory in which to run
        Returns:
          A FixerResult object
        """
        raise NotImplementedError(self.run)


class ScriptFixer(Fixer):
    """A fixer that is implemented as a shell/python/etc script."""

    def __init__(self, tag, script_path):
        super(ScriptFixer, self).__init__([tag])
        self.script_path = script_path

    def __repr__(self):
        return "ScriptFixer(%r, %r)" % (self.lintian_tags[0], self.script_path)

    def run(self, basedir):
        note('Running fixer %r on %s', self, basedir)
        p = subprocess.Popen(self.script_path, cwd=basedir,
                             stdout=subprocess.PIPE, stderr=sys.stderr)
        (description, err) = p.communicate("")
        if p.returncode != 0:
            raise ScriptFailed("Script %s failed with error code %d" % (
                    self.script_path, p.returncode))
        if not description:
            raise DescriptionMissing(self)
        description = description.decode('utf-8')
        lines = []
        fixed_tags = []
        for line in description.splitlines():
            # TODO(jelmer): Do this in a slighly less hackish manner
            if line.startswith('Fixed-Lintian-Tags: '):
                fixed_tags = line.split(':', 1)[1].strip().split(',')
            else:
                lines.append(line)
        return FixerResult(description, fixed_tags)


def find_fixers_dir():
    """Find the local directory with lintian fixer scripts."""
    local_dir = os.path.join(
        os.path.dirname(__file__), '..', 'fixers', 'lintian')
    if os.path.isdir(local_dir):
        return local_dir
    import pkg_resources
    resource_dir = pkg_resources.resource_filename(
        __name__, 'lintian-brush/fixers/lintian')
    if os.path.isdir(resource_dir):
        return resource_dir
    # Urgh.
    return '/usr/share/lintian-brush/fixers/lintian'


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
        if n.endswith("~") or n.startswith("."):
            continue
        tag = os.path.splitext(n)[0]
        path = os.path.join(fixers_dir, n)
        if os.path.isdir(path):
            continue
        yield ScriptFixer(tag, path)


def run_lintian_fixer(local_tree, fixer, update_changelog=True):
    """Run a lintian fixer on a tree.

    Args:
      local_tree: WorkingTree object
      fixer: Fixer object to apply
      update_changelog: Whether to add a new entry to the changelog
    Returns:
      summary of the changes
    """
    # Just check there are no changes to begin with
    if list(local_tree.iter_changes(local_tree.basis_tree())):
        raise AssertionError("Local tree %s has changes" % local_tree.basedir)
    try:
        result = fixer.run(local_tree.basedir)
    except BaseException:
        revert(local_tree, local_tree.branch.basis_tree(), None)
        deletables = list(iter_deletables(
            local_tree, unknown=True, ignored=True, detritus=True))
        delete_items(deletables)
        raise
    unknowns = list(local_tree.unknowns())
    if unknowns:
        # Urgh.
        local_tree.add(
            [f for f in unknowns
             if not os.path.basename(f).startswith('sed')])
    if local_tree.supports_setting_file_ids():
        RenameMap.guess_renames(
            local_tree.basis_tree(), local_tree, dry_run=False)

    summary = result.description.splitlines()[0]

    if not list(local_tree.iter_changes(local_tree.basis_tree())):
        raise NoChanges("Script didn't make any changes")

    if update_changelog:
        subprocess.check_call(
            ["dch", "--no-auto-nmu", summary], cwd=local_tree.basedir)

    description = result.description
    for tag in result.fixed_lintian_tags:
        description += "\n"
        description += "Fixes lintian: %s\n" % tag
        description += ("See https://lintian.debian.org/tags/%s.html "
                        "for more details.\n") % tag

    local_tree.commit(description, allow_pointless=False)
    # TODO(jelmer): Run sbuild & verify lintian warning is gone?
    return result.fixed_lintian_tags, summary


def run_lintian_fixers(local_tree, fixers, update_changelog=True):
    """Run a set of lintian fixers on a tree.

    Args:
      local_tree: WorkingTree object
      fixers: A set of Fixer objects
      update_changelog: Whether to add an entry to the changelog
    Returns:
      List of tuples with (lintian-tag, description)
    """
    ret = []
    for fixer in fixers:
        try:
            fixed_lintian_tags, summary = run_lintian_fixer(
                    local_tree, fixer, update_changelog)
        except ScriptFailed:
            note('Fixer %r failed to run', fixer)
        except NoChanges:
            pass
        else:
            ret.append((fixed_lintian_tags, summary))
    return ret
