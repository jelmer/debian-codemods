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
import json
import logging
import os
from typing import List, Tuple, Optional, Dict, Callable

from breezy.commit import PointlessCommit

from debmutate.control import PkgRelation
from debmutate.deb822 import ChangeConflict
from debmutate.debhelper import MaintscriptEditor
from debmutate.reformatting import FormattingUnpreservable

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
from . import (
    get_committer,
    control_files_in_root,
    control_file_present,
    NotDebianPackage,
    is_debcargo_package,
    )
from .debhelper import drop_obsolete_maintscript_entries


DEFAULT_VALUE_MULTIARCH_HINT = 30


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


def depends_obsolete(latest_version, kind, req_version):
    req_version = Version(req_version)
    if kind == ">=":
        return latest_version >= req_version
    elif kind == ">>":
        return latest_version > req_version
    elif kind == "=":
        return False
    return False


def conflict_obsolete(latest_version, kind, req_version):
    req_version = Version(req_version)
    if kind == "<<":
        return latest_version >= req_version
    elif kind in ("<=", "="):
        return latest_version > req_version
    return False


async def _package_version(source, release):
    from .udd import connect_udd_mirror

    conn = await connect_udd_mirror()
    version = await conn.fetchval(
        "select version from sources where source = $1 and release = $2",
        source,
        release,
    )
    if version is not None:
        return Version(version)
    return None


def package_version(source, release):
    loop = asyncio.get_event_loop()
    return loop.run_until_complete(_package_version(source, release))


def drop_obsolete_depends(entry, upgrade_release: str):
    ors = []
    dropped = []
    for pkgrel in entry:
        if pkgrel.version is not None:
            compat_version = package_version(pkgrel.name, upgrade_release)
            logging.debug(
                "Relation: %s. Upgrade release %s has %r ",
                pkgrel, upgrade_release, compat_version)
            if compat_version is not None and depends_obsolete(
                compat_version, *pkgrel.version
            ):
                pkgrel.version = None
                dropped.append(pkgrel)
        ors.append(pkgrel)
    return ors, dropped


def drop_obsolete_conflicts(entry: List[PkgRelation], upgrade_release: str):
    ors = []
    dropped = []
    for pkgrel in entry:
        if pkgrel.version is not None:
            compat_version = package_version(pkgrel.name, upgrade_release)
            if compat_version is not None and conflict_obsolete(
                compat_version, *pkgrel.version
            ):
                dropped.append(pkgrel)
                continue
        ors.append(pkgrel)
    return ors, dropped


def update_depends(base, field, upgrade_release):
    return filter_relations(
        base, field,
        lambda oldrelation: drop_obsolete_depends(oldrelation, upgrade_release))


def _relations_empty(rels):
    for ws1, rel, ws2 in rels:
        if rel:
            return False
    return True


RelationsCallback = Callable[[List[PkgRelation]], Tuple[List[PkgRelation], List[PkgRelation]]]


def filter_relations(base, field: str, cb: RelationsCallback):
    """Update a relations field."""
    try:
        old_contents = base[field]
    except KeyError:
        return []

    oldrelations = parse_relations(old_contents)
    newrelations = []

    changed = []
    for i, (ws1, oldrelation, ws2) in enumerate(oldrelations):
        relation, dropped = cb(oldrelation)
        changed.extend([d.name for d in dropped])
        if relation == oldrelation or relation:
            newrelations.append((ws1, relation, ws2))
        elif i == 0 and len(oldrelations) > 1:
            # If the first item is removed, then copy the spacing to the next
            # item
            oldrelations[1] = (ws1, oldrelations[1][1], ws2)

    if changed:
        if _relations_empty(newrelations):
            del base[field]
        else:
            base[field] = format_relations(newrelations)
        return changed
    return []


def update_conflicts(base, field: str, upgrade_release: str):
    return filter_relations(
        base, field,
        lambda oldrelation: drop_obsolete_conflicts(oldrelation, upgrade_release))


def drop_old_source_relations(source, compat_release) -> List[Tuple[str, List[str], str]]:
    ret = []
    for field in [
        "Build-Depends",
        "Build-Depends-Indep",
        "Build-Depends-Arch",
    ]:
        packages = update_depends(source, field, compat_release)
        if packages:
            ret.append((field, packages, compat_release))
    for field in ["Build-Conflicts", "Build-Conflicts-Indep", "Build-Conflicts-Arch"]:
        packages = update_conflicts(source, field, compat_release)
        if packages:
            ret.append((field, packages, compat_release))
    return ret


def drop_old_binary_relations(binary, upgrade_release) -> List[Tuple[str, List[str], str]]:
    ret = []
    for field in ["Depends", "Breaks", "Suggests", "Recommends", "Pre-Depends"]:
        packages = update_depends(binary, field, upgrade_release)
        if packages:
            ret.append((field, packages, upgrade_release))

    for field in ["Conflicts", "Replaces", "Breaks"]:
        packages = update_conflicts(binary, field, upgrade_release)
        if packages:
            ret.append((field, packages, upgrade_release))

    return ret


def drop_old_relations(editor, compat_release, upgrade_release) -> List[Tuple[Optional[str], List[Tuple[str, List[str], str]]]]:
    dropped: List[Tuple[Optional[str], List[Tuple[str, List[str], str]]]] = []
    source_dropped = []
    try:
        check_generated_file(editor.path)
    except GeneratedFile as e:
        uses_cdbs = (
            e.template_path is not None
            and guess_template_type(e.template_path) == "cdbs"
        )
    else:
        uses_cdbs = False
    if not uses_cdbs:
        source_dropped.extend(drop_old_source_relations(editor.source, compat_release))
    if source_dropped:
        dropped.append((None, source_dropped))

    for binary in editor.binaries:
        binary_dropped = drop_old_binary_relations(binary, upgrade_release)
        if binary_dropped:
            dropped.append((binary["Package"], binary_dropped))

    return dropped


def update_maintscripts(wt, subpath, package, upgrade_release, allow_reformatting=False):
    ret = []
    for entry in os.scandir(wt.abspath(os.path.join(subpath))):
        if not (entry.name == "maintscript" or entry.name.endswith(".maintscript")):
            continue
        with MaintscriptEditor(entry.path, allow_reformatting=allow_reformatting) as editor:
            def can_drop(p, v):
                compat_version = package_version(p or package, upgrade_release)
                return compat_version is not None and compat_version > v
            removed = drop_obsolete_maintscript_entries(editor, can_drop)
            if removed:
                ret.append((os.path.join(subpath, entry.name), removed))
    return ret


def name_list(packages):
    if not packages:
        raise ValueError(packages)
    std = list(sorted(set(packages)))
    if len(std) == 1:
        return std[0]
    return ", ".join(std[:-1]) + " and " + std[-1]


class ScrubObsoleteResult(object):
    def __init__(self, specific_files, maintscript_removed, control_removed):
        self.specific_files = specific_files
        self.maintscript_removed = maintscript_removed
        self.control_removed = control_removed

    def __bool__(self):
        return bool(self.control_removed) or bool(self.maintscript_removed)

    def value(self):
        value = DEFAULT_VALUE_MULTIARCH_HINT
        for para, changes, release in self.control_removed:
            for field, packages in changes:
                value += len(packages) * 2
        for path, removed, release in self.maintscript_removed:
            value += len(removed)
        return value

    def itemized(self) -> Dict[str, List[str]]:
        summary: Dict[str, List[str]] = {}
        for para, changes in self.control_removed:
            for field, packages, release in changes:
                if para:
                    summary.setdefault(release, []).append(
                        "%s: Drop versioned constraint on %s in %s."
                        % (para, name_list(packages), field)
                    )
                else:
                    summary.setdefault(release, []).append(
                        "%s: Drop versioned constraint on %s."
                        % (field, name_list(packages))
                    )
        if self.maintscript_removed:
            total_entries = sum(
                [len(entries) for name, entries, release in self.maintscript_removed])
            summary.setdefault(self.maintscript_removed[0][2], []).append(
                "Remove %d maintscript entries from %d files." % (
                    total_entries, len(self.maintscript_removed))
            )
        return summary


def _scrub_obsolete(wt, debian_path, compat_release, upgrade_release, allow_reformatting):
    specific_files = []
    control_path = os.path.join(debian_path, "control")
    try:
        with ControlEditor(
                wt.abspath(control_path),
                allow_reformatting=allow_reformatting) as editor:
            specific_files.append(control_path)
            package = editor.source["Source"]
            control_removed = drop_old_relations(editor, compat_release, upgrade_release)
    except FileNotFoundError:
        if wt.has_filename(os.path.join(debian_path, "debcargo.toml")):
            control_removed = []
        else:
            raise NotDebianPackage(wt, debian_path)

    maintscript_removed = []
    for path, removed in update_maintscripts(wt, debian_path, package, upgrade_release, allow_reformatting):
        if removed:
            maintscript_removed.append((path, removed, upgrade_release))
            specific_files.append(path)

    return ScrubObsoleteResult(
        specific_files=specific_files,
        control_removed=control_removed,
        maintscript_removed=maintscript_removed,
    )


def scrub_obsolete(
        wt, subpath, compat_release, upgrade_release, update_changelog=None,
        allow_reformatting=False):
    from breezy.commit import NullCommitReporter

    if control_files_in_root(wt, subpath):
        debian_path = subpath
    else:
        debian_path = os.path.join(subpath, 'debian')

    result = _scrub_obsolete(
        wt, debian_path, compat_release, upgrade_release, allow_reformatting)

    if not result:
        return result

    specific_files = list(result.specific_files)
    summary = result.itemized()

    changelog_path = os.path.join(debian_path, "changelog")

    if update_changelog is None:
        from .detect_gbp_dch import guess_update_changelog
        from debian.changelog import Changelog

        with wt.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)

        dch_guess = guess_update_changelog(wt, debian_path, cl)
        if dch_guess:
            update_changelog = dch_guess[0]
            _note_changelog_policy(update_changelog, dch_guess[1])
        else:
            # Assume we should update changelog
            update_changelog = True

    if update_changelog:
        lines = []
        for release, entries in summary.items():
            lines.append("Remove constraints unnecessary since %s:" % release)
            lines.extend(["+ " + line for line in entries])
        add_changelog_entry(wt, changelog_path, lines)
        specific_files.append(changelog_path)

    lines = []
    for release, entries in summary.items():
        lines.extend(["Remove constraints unnecessary since %s" % release, ""])
        lines.extend(["* " + line for line in entries])
    lines.extend(["", "Changes-By: deb-scrub-obsolete"])

    committer = get_committer(wt)

    try:
        wt.commit(
            specific_files=specific_files,
            message="\n".join(lines),
            allow_pointless=False,
            reporter=NullCommitReporter(),
            committer=committer,
        )
    except PointlessCommit:
        pass

    return result


def report_fatal(code, description):
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'description': description}, f)
    logging.fatal('%s', description)


def report_okay(code, description):
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'description': description}, f)
    logging.info('%s', description)


def main():  # noqa: C901
    import argparse
    from breezy.workingtree import WorkingTree
    import breezy  # noqa: E402

    breezy.initialize()
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    from . import (
        check_clean_tree,
        PendingChanges,
        version_string,
    )
    from .config import Config

    parser = argparse.ArgumentParser(prog="deb-scrub-obsolete")
    parser.add_argument(
        "--directory",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )
    parser.add_argument(
        "--upgrade-release",
        metavar="UPGRADE-RELEASE",
        help="Release to allow upgrading from.",
        default="oldstable",
    )
    parser.add_argument(
        '--compat-release',
        metavar='COMPAT-RELEASE',
        help="Release to allow building on.",
        default=None)
    parser.add_argument(
        "--no-update-changelog",
        action="store_false",
        default=None,
        dest="update_changelog",
        help="do not update the changelog",
    )
    parser.add_argument(
        "--update-changelog",
        action="store_true",
        dest="update_changelog",
        help="force updating of the changelog",
        default=None,
    )
    parser.add_argument(
        "--allow-reformatting",
        default=None,
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument(
        "--identity",
        help="Print user identity that would be used when committing",
        action="store_true",
        default=False,
    )
    parser.add_argument(
        "--debug", help="Describe all considerd changes.", action="store_true"
    )

    args = parser.parse_args()

    wt, subpath = WorkingTree.open_containing(args.directory)
    if args.identity:
        logging.info('%s', get_committer(wt))
        return 0

    try:
        check_clean_tree(wt, wt.basis_tree(), subpath)
    except PendingChanges:
        logging.info("%s: Please commit pending changes first.", wt.basedir)
        return 1

    import distro_info
    debian_info = distro_info.DebianDistroInfo()
    upgrade_release = debian_info.codename(args.upgrade_release)

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format='%(message)s')

    update_changelog = args.update_changelog
    allow_reformatting = args.allow_reformatting
    if args.compat_release:
        compat_release = debian_info.codename(args.compat_release)
    else:
        compat_release = None
    try:
        cfg = Config.from_workingtree(wt, subpath)
    except FileNotFoundError:
        pass
    else:
        if update_changelog is None:
            update_changelog = cfg.update_changelog()
        if allow_reformatting is None:
            allow_reformatting = cfg.allow_reformatting()
        if compat_release is None:
            compat_release = cfg.compat_release()

    if compat_release is None:
        compat_release = debian_info.stable()

    logging.info(
        "Removing run time constraints unnecessary since %s"
        " and build time constraints unnecessary since %s",
        upgrade_release, compat_release)

    if allow_reformatting is None:
        allow_reformatting = False

    if is_debcargo_package(wt, subpath):
        report_fatal("nothing-to-do", "Package uses debcargo")
        return 1
    elif not control_file_present(wt, subpath):
        report_fatal("missing-control-file", "Unable to find debian/control")
        return 1

    try:
        result = scrub_obsolete(
            wt, subpath, compat_release, upgrade_release,
            update_changelog=args.update_changelog,
            allow_reformatting=allow_reformatting
        )
    except FormattingUnpreservable as e:
        report_fatal(
            "formatting-unpreservable",
            "unable to preserve formatting while editing %s" % e.path,
        )
        return 1
    except GeneratedFile as e:
        report_fatal(
            "generated-file", "unable to edit generated file: %r" % e
        )
        return 1
    except NotDebianPackage:
        report_fatal('not-debian-package', 'Not a Debian package.')
        return 1
    except ChangeConflict as e:
        report_fatal('change-conflict', 'Generated file changes conflict: %s' % e)
        return 1

    if not result:
        report_okay("nothing-to-do", "no obsolete constraints")
        return 0

    if os.environ.get("SVP_API") == "1":
        with open(os.environ["SVP_RESULT"], "w") as f:
            json.dump({
                "description": "Remove constraints unnecessary since %s."
                % upgrade_release,
                "value": result.value(),
                "context": {
                    "specific_files": result.specific_files,
                    "maintscript_removed": result.maintscript_removed,
                    "control_removed": result.control_removed,
                }
            }, f)

    logging.info("Scrub obsolete settings.")
    for release, lines in result.itemized().items():
        for line in lines:
            logging.info("* %s", line)

    return 0


if __name__ == "__main__":
    import sys

    sys.exit(main())
