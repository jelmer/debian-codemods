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

"""Utility for dropping unnecessary constraints."""

import asyncio
import logging
import os

from breezy.commit import PointlessCommit
from breezy.trace import note

try:
    from debmutate.debhelper import MaintscriptEditor
except ImportError:
    MaintscriptEditor = None


from debian.changelog import Version

from debmutate.control import (
    ControlEditor,
    parse_relations,
    format_relations,
    guess_template_type,
    )

from debmutate.reformatting import (
    check_generated_file,
    GeneratedFile,
    )

from .changelog import add_changelog_entry
from . import get_committer


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


def drop_old_maintscript(editor, package_name, upgrade_release):
    remove = []
    for i, entry in enumerate(list(editor.lines)):
        if isinstance(entry, str):
            continue
        prior_version = getattr(entry, 'prior_version', None)
        if prior_version is None:
            continue
        package = entry.package or package_name
        compat_version = package_version(package, upgrade_release)
        if compat_version is None:
            continue
        if compat_version > prior_version:
            remove.append(i)
    removed = []
    for i in reversed(remove):
        removed.append(editor.lines[i])
        del editor.lines[i]
    return removed


def depends_obsolete(latest_version, kind, req_version):
    req_version = Version(req_version)
    if kind == '>=':
        return latest_version >= req_version
    elif kind == '>>':
        return latest_version > req_version
    elif kind == '=':
        return False
    return False


def conflict_obsolete(latest_version, kind, req_version):
    req_version = Version(req_version)
    if kind == '<<':
        return latest_version >= req_version
    elif kind in ('<=', '='):
        return latest_version > req_version
    return False


async def _package_version(source, release):
    from .udd import connect_udd_mirror
    conn = await connect_udd_mirror()
    version = await conn.fetchval(
        "select version from sources where source = $1 and release = $2",
        source, release)
    if version is not None:
        return Version(version)
    return None


def package_version(source, upgrade_release):
    loop = asyncio.get_event_loop()
    return loop.run_until_complete(_package_version(source, upgrade_release))


def drop_obsolete_depends(entry, upgrade_release):
    ors = []
    dropped = []
    for pkgrel in entry:
        if pkgrel.version is not None:
            logging.debug('Relation: %s', pkgrel)
            compat_version = package_version(pkgrel.name, upgrade_release)
            if (compat_version is not None and
                    depends_obsolete(compat_version, *pkgrel.version)):
                pkgrel.version = None
                dropped.append(pkgrel)
        ors.append(pkgrel)
    return ors, dropped


def drop_obsolete_conflicts(entry, upgrade_release):
    ors = []
    dropped = []
    for pkgrel in entry:
        if pkgrel.version is not None:
            compat_version = package_version(pkgrel.name, upgrade_release)
            if (compat_version is not None and
                    conflict_obsolete(compat_version, *pkgrel.version)):
                dropped.append(pkgrel)
                continue
        ors.append(pkgrel)
    return ors, dropped


def update_depends(base, field, upgrade_release):
    try:
        old_contents = base[field]
    except KeyError:
        return []

    changed = []
    newrelations = []
    for ws1, oldrelation, ws2 in parse_relations(old_contents):
        relation, dropped = drop_obsolete_depends(oldrelation, upgrade_release)
        changed.extend([d.name for d in dropped])
        if relation:
            newrelations.append((ws1, relation, ws2))

    if changed:
        if relations_empty(newrelations):
            del base[field]
        else:
            base[field] = format_relations(newrelations)
        return changed
    return []


def relations_empty(rels):
    for ws1, rel, ws2 in rels:
        if rel:
            return False
    return True


def update_conflicts(base, field, upgrade_release):
    try:
        old_contents = base[field]
    except KeyError:
        return []

    changed = []
    newrelations = []
    for ws1, oldrelation, ws2 in parse_relations(old_contents):
        relation, dropped = drop_obsolete_conflicts(
            oldrelation, upgrade_release)
        changed.extend([d.name for d in dropped])
        newrelations.append((ws1, relation, ws2))

    if changed:
        if relations_empty(newrelations):
            del base[field]
        else:
            base[field] = format_relations(newrelations)
        return changed
    return []


def drop_old_source_relations(source, upgrade_release):
    ret = []
    for field in [
            'Build-Depends', 'Build-Depends-Indep', 'Build-Depends-Arch',
            ]:
        packages = update_depends(source, field, upgrade_release)
        if packages:
            ret.append((field, packages))
    for field in [
            'Build-Conflicts', 'Build-Conflicts-Indep',
            'Build-Conflicts-Arch']:
        packages = update_conflicts(source, field, upgrade_release)
        if packages:
            ret.append((field, packages))
    return ret


def drop_old_binary_relations(binary, upgrade_release):
    ret = []
    for field in [
            'Depends', 'Breaks', 'Suggests', 'Recommends', 'Pre-Depends']:
        packages = update_depends(binary, field, upgrade_release)
        if packages:
            ret.append((field, packages))

    for field in ['Conflicts', 'Replaces', 'Breaks']:
        packages = update_conflicts(binary, field, upgrade_release)
        if packages:
            ret.append((field, packages))

    return ret


def drop_old_relations(editor, upgrade_release):
    dropped = []
    source_dropped = []
    try:
        check_generated_file(editor.path)
    except GeneratedFile as e:
        uses_cdbs = (
            e.template_path is not None and
            guess_template_type(e.template_path) == 'cdbs')
    else:
        uses_cdbs = False
    if not uses_cdbs:
        source_dropped.extend(
                drop_old_source_relations(editor.source, upgrade_release))
    if source_dropped:
        dropped.append((None, source_dropped))

    for binary in editor.binaries:
        binary_dropped = drop_old_binary_relations(binary, upgrade_release)
        if binary_dropped:
            dropped.append((binary['Package'], binary_dropped))

    return dropped


def update_maintscripts(wt, path, package, upgrade_release):
    ret = []
    for entry in os.scandir(wt.abspath(os.path.join(path, 'debian'))):
        if not (entry.name == 'maintscript' or
                entry.name.endswith('.maintscript')):
            continue
        with MaintscriptEditor(entry.path) as editor:
            removed = drop_old_maintscript(editor, package, upgrade_release)
            if removed:
                ret.append((os.path.join(path, 'debian', entry.name), removed))
    return ret


def name_list(packages):
    if len(packages) == 1:
        return packages[0]
    std = list(sorted(set(packages)))
    return ', '.join(std[:-1]) + ' and ' + std[-1]


class ScrubObsoleteResult(object):

    def __init__(self, specific_files, maintscript_removed,
                 control_removed):
        self.specific_files = specific_files
        self.maintscript_removed = maintscript_removed
        self.control_removed = control_removed

    def __bool__(self):
        return bool(self.control_removed) or bool(self.maintscript_removed)

    def itemized(self):
        summary = []
        for para, changes in self.control_removed:
            for field, packages in changes:
                if para:
                    summary.append(
                        '%s: Drop versioned constraint on %s in %s.' % (
                         para, name_list(packages), field))
                else:
                    summary.append(
                        '%s: Drop versioned constraint on %s.' % (
                         field, name_list(packages)))
        if self.maintscript_removed:
            summary.append('Remove %d maintscript entries.' %
                           len(self.maintscript_removed))
        return summary


def _scrub_obsolete(wt, subpath, upgrade_release):
    specific_files = []
    control_path = os.path.join(subpath, 'debian/control')
    try:
        with ControlEditor(wt.abspath(control_path)) as editor:
            specific_files.append(control_path)
            package = editor.source['Source']
            control_removed = drop_old_relations(editor, upgrade_release)
    except FileNotFoundError:
        if wt.has_filename(os.path.join(subpath, 'debian/debcargo.toml')):
            return ScrubObsoleteResult([], [], [])
        raise

    maintscript_removed = []
    for path, removed in update_maintscripts(
            wt, subpath, package, upgrade_release):
        if removed:
            maintscript_removed.append((path, removed))
            specific_files.append(path)

    return ScrubObsoleteResult(
        specific_files=specific_files,
        control_removed=control_removed,
        maintscript_removed=maintscript_removed)


def scrub_obsolete(wt, subpath, upgrade_release, update_changelog=None):
    from breezy.commit import NullCommitReporter
    result = _scrub_obsolete(wt, subpath, upgrade_release)

    if not result:
        return result

    specific_files = list(result.specific_files)
    summary = result.itemized()

    changelog_path = os.path.join(subpath, 'debian/changelog')

    if update_changelog is None:
        from .detect_gbp_dch import guess_update_changelog
        from debian.changelog import Changelog
        with wt.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)

        dch_guess = guess_update_changelog(wt, subpath, cl)
        if dch_guess:
            update_changelog = dch_guess[0]
            _note_changelog_policy(update_changelog, dch_guess[1])
        else:
            # Assume we should update changelog
            update_changelog = True

    if update_changelog:
        add_changelog_entry(
            wt, changelog_path,
            ['Remove constraints unnecessary since %s:' %
             upgrade_release] + ['+ ' + line for line in summary])
        specific_files.append(changelog_path)

    message = '\n'.join([
        'Remove constraints unnecessary since %s.' % upgrade_release, ''] +
        ['* ' + line for line in summary] +
        ['', 'Changes-By: deb-scrub-obsolete'])

    committer = get_committer(wt)

    try:
        wt.commit(
            specific_files=specific_files,
            message=message,
            allow_pointless=False,
            reporter=NullCommitReporter(),
            committer=committer)
    except PointlessCommit:
        pass

    return result


def main():
    import argparse
    from breezy.workingtree import WorkingTree
    import breezy  # noqa: E402
    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402
    from breezy.trace import note  # note: E402

    from . import (
        check_clean_tree,
        PendingChanges,
        version_string,
        )
    from .config import Config

    parser = argparse.ArgumentParser(prog='drop-backwards-compat')
    parser.add_argument(
        '--directory', metavar='DIRECTORY', help='directory to run in',
        type=str, default='.')
    parser.add_argument(
        '--upgrade-release', metavar='UPGRADE-RELEASE',
        help='Release to allow upgrading from.', default='oldstable')
    parser.add_argument(
        '--no-update-changelog', action="store_false", default=None,
        dest="update_changelog", help="do not update the changelog")
    parser.add_argument(
        '--update-changelog', action="store_true", dest="update_changelog",
        help="force updating of the changelog", default=None)
    parser.add_argument(
        '--version', action='version', version='%(prog)s ' + version_string)
    parser.add_argument(
        '--identity',
        help='Print user identity that would be used when committing',
        action='store_true', default=False)
    parser.add_argument(
        '--debug',
        help='Describe all considerd changes.',
        action='store_true')

    args = parser.parse_args()

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        note(get_committer(wt))
        return 0

    try:
        check_clean_tree(wt, wt.basis_tree(), subpath)
    except PendingChanges:
        note("%s: Please commit pending changes first.", wt.basedir)
        return 1

    import distro_info
    debian_info = distro_info.DebianDistroInfo()
    upgrade_release = debian_info.codename(args.upgrade_release)

    note('Removing constraints unnecessary since %s', upgrade_release)

    if args.debug:
        logging.basicConfig()
        logging.getLogger().setLevel(logging.DEBUG)

    update_changelog = args.update_changelog
    try:
        cfg = Config.from_workingtree(wt, subpath)
    except FileNotFoundError:
        pass
    else:
        if update_changelog is None:
            update_changelog = cfg.update_changelog()

    result = scrub_obsolete(
        wt, subpath, upgrade_release,
        update_changelog=args.update_changelog)
    if not result:
        return 0


if __name__ == '__main__':
    import sys
    sys.exit(main())
