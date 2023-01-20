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
from contextlib import suppress
import json
import logging
import os
from typing import List, Tuple, Optional, Dict, Callable, Union

from breezy.commit import PointlessCommit, NullCommitReporter
from breezy.workingtree import WorkingTree

from debmutate.control import PkgRelation
from debmutate.deb822 import ChangeConflict
from debmutate.debhelper import MaintscriptEditor
from debmutate.reformatting import FormattingUnpreservable

from debian.changelog import Version
from debian.deb822 import Deb822Dict

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


class Action:
    """Action."""

    def __init__(self, rel):
        self.rel = rel

    def __repr__(self):
        return f"<{type(self).__name__}({PkgRelation.str(self.rel)!r})>"

    def __eq__(self, other):
        return isinstance(other, type(self)) and self.rel == other.rel

    def json(self):
        raise NotImplementedError(self.json)


class DropEssential(Action):
    """Drop dependency on essential package."""

    def __str__(self):
        return "Drop dependency on essential package %s" % (
            PkgRelation.str(self.rel))

    def json(self):
        return ("drop-essential", PkgRelation.str(self.rel))


class DropMinimumVersion(Action):
    """Drop minimumversion."""

    def __str__(self):
        return "Drop versioned constraint on %s" % PkgRelation.str(self.rel)

    def json(self):
        return ("drop-minimum-version", PkgRelation.str(self.rel))


class DropTransition(Action):
    """Drop dependency on dummy transitional package."""

    def __str__(self):
        return (
            "Drop dependency on transitional package %s" %
            PkgRelation.str(self.rel))

    def json(self):
        return ("drop-transitional", PkgRelation.str(self.rel))


class ReplaceTransition(Action):
    """Replace dependency on dummy transitional package."""

    def __init__(self, rel, replacement):
        super().__init__(rel)
        self.replacement = replacement

    def __str__(self):
        return (
            "Replace dependency on transitional package %s "
            "with replacement %s" % (
                PkgRelation.str(self.rel), name_list([
                    PkgRelation.str(p) for p in self.replacement])))

    def json(self):
        return ("inline-transitional", PkgRelation.str(self.rel),
                [PkgRelation.str(p) for p in self.replacement])

    def __repr__(self):
        return "<{}({!r}, {!r})>".format(
            type(self).__name__, PkgRelation.str(self.rel),
            [PkgRelation(p) for p in self.replacement])

    def __eq__(self, other):
        return (
            isinstance(other, type(self)) and self.rel == other.rel
            and self.replacement == other.replacement)


class DropObsoleteConflict(Action):
    """Drop conflict with obsolete package."""

    def __str__(self):
        return "Drop conflict with removed package %s" % (
            PkgRelation.str(self.rel))

    def json(self):
        return ("drop-obsolete-conflict", PkgRelation.str(self.rel))


def depends_obsolete(
        latest_version: Version, kind: str, req_version: Version) -> bool:
    req_version = Version(req_version)
    if kind == ">=":  # noqa: SIM116
        return latest_version >= req_version
    elif kind == ">>":
        return latest_version > req_version
    elif kind == "=":
        return False
    return False


def conflict_obsolete(
        latest_version: Version, kind: str, req_version: Version):
    req_version = Version(req_version)
    if kind == "<<":
        return latest_version >= req_version
    elif kind in ("<=", "="):
        return latest_version > req_version
    return False


class UddTimeout(Exception):
    """Timeout while connecting to UDD."""


async def _package_version(package: str, release: str) -> Optional[Version]:
    from .udd import connect_udd_mirror

    try:
        async with await connect_udd_mirror() as conn:
            version = await conn.fetchval(
                "SELECT version FROM packages "
                "WHERE package = $1 AND release = $2",
                package, release)
    except asyncio.TimeoutError as exc:
        raise UddTimeout() from exc
    if version is not None:
        return Version(version)
    return None


async def _package_provides(
        package: str, release: str) -> Optional[List[PkgRelation]]:
    from .udd import connect_udd_mirror

    async with await connect_udd_mirror() as conn:
        provides = await conn.fetchval(
            "SELECT provides FROM packages "
            "WHERE package = $1 AND release = $2",
            package, release)
    if provides is not None:
        return [r for sublist in parse_relations(provides) for r in sublist[1]]
    return None


async def _package_essential(package: str, release: str) -> bool:
    from .udd import connect_udd_mirror

    async with await connect_udd_mirror() as conn:
        return await conn.fetchval(
            "SELECT (essential = 'yes') FROM packages "
            "WHERE package = $1 AND release = $2",
            package, release)


async def _package_build_essential(package: str, release: str) -> bool:
    from .udd import connect_udd_mirror

    async with await connect_udd_mirror() as conn:
        depends = await conn.fetchval(
            "select depends from packages where package = $1 and release = $2",
            'build-essential', release)

    build_essential = set()
    for _ws1, rel, _ws2 in parse_relations(depends):
        build_essential.update([r.name for r in rel])
    return package in build_essential


async def _fetch_transitions(release: str) -> Dict[str, str]:
    from .udd import connect_udd_mirror
    from .dummy_transitional import find_dummy_transitional_packages

    ret = {}
    async with await connect_udd_mirror() as conn:
        for transition in (await find_dummy_transitional_packages(
                conn, release)).values():
            ret[transition.from_name] = transition.to_expr
    return ret


class PackageChecker:

    def __init__(self, release: str, build: bool):
        self.release = release
        self.build = build
        self._transitions: Optional[Dict[str, str]] = None

    def package_version(self, package: str) -> Optional[Version]:
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(_package_version(package, self.release))

    def replacement(self, package: str) -> Optional[str]:
        if self._transitions is None:
            loop = asyncio.get_event_loop()
            self._transitions = loop.run_until_complete(
                _fetch_transitions(self.release))
        return self._transitions.get(package)

    def package_provides(self, package):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(
            _package_provides(package, self.release))

    def is_essential(self, package: str) -> bool:
        loop = asyncio.get_event_loop()
        if self.build and loop.run_until_complete(
                _package_build_essential(package, self.release)):
            return True
        return loop.run_until_complete(
            _package_essential(package, self.release))


def drop_obsolete_depends(
        entry: List[PkgRelation], checker: PackageChecker,
        keep_minimum_versions: bool = False):
    ors = []
    actions: List[Action] = []
    for pkgrel in entry:
        newrel = pkgrel
        replacement = checker.replacement(pkgrel.name)
        if replacement:
            parsed_replacement = PkgRelation.parse(replacement)
            if len(parsed_replacement) != 1:
                logging.warning(
                    'Unable to replace multi-package %r', replacement)
            else:
                newrel = parsed_replacement[0]
                if newrel in entry:
                    actions.append(DropTransition(pkgrel))
                    continue
                actions.append(ReplaceTransition(pkgrel, parsed_replacement))
        elif pkgrel.version is not None and pkgrel.name != 'debhelper':
            compat_version = checker.package_version(pkgrel.name)
            logging.debug(
                "Relation: %s. Upgrade release %s has %r ",
                pkgrel, checker.release, compat_version)
            if compat_version is not None and depends_obsolete(
                compat_version, *pkgrel.version
            ):
                # If the package is essential, we don't need to maintain a
                # dependency on it.
                if checker.is_essential(pkgrel.name):
                    actions.append(DropEssential(pkgrel))
                    return [], actions
                if not keep_minimum_versions:
                    newrel = PkgRelation.parse(pkgrel.str())[0]
                    newrel.version = None
                    actions.append(DropMinimumVersion(pkgrel))
        ors.append(newrel)
    if not actions:
        return ors, []

    deduped = []
    for rel in ors:
        if rel not in deduped:
            deduped.append(rel)

    # TODO: if dropped: Check if any ors are implied by existing other
    # dependencies
    return deduped, actions


def drop_obsolete_conflicts(checker: PackageChecker, entry: List[PkgRelation]):
    ors = []
    actions: List[Action] = []
    for pkgrel in entry:
        if pkgrel.version is not None:
            compat_version = checker.package_version(pkgrel.name)
            if compat_version is not None and conflict_obsolete(
                compat_version, *pkgrel.version
            ):
                actions.append(DropObsoleteConflict(pkgrel))
                continue
        ors.append(pkgrel)
    return ors, actions


def update_depends(
        base: Deb822Dict, field: str, checker: PackageChecker,
        keep_minimum_versions: bool = False) -> List[Action]:
    return filter_relations(
        base, field,
        lambda oldrelation: drop_obsolete_depends(
            oldrelation, checker, keep_minimum_versions=keep_minimum_versions))


def _relations_empty(rels):
    return all(not rel for _ws1, rel, _ws2 in rels)


RelationsCallback = Callable[
    [List[PkgRelation]], Tuple[List[PkgRelation], List[Action]]]


def filter_relations(
        base: Union[Deb822Dict, Dict[str, str]],
        field: str, cb: RelationsCallback) -> List[Action]:
    """Update a relations field."""
    try:
        old_contents = base[field]
    except KeyError:
        return []

    oldrelations = parse_relations(old_contents)
    newrelations = []

    all_actions: List[Action] = []
    for i, (ws1, oldrelation, ws2) in enumerate(oldrelations):
        relation, actions = cb(oldrelation)
        all_actions.extend(actions)
        if relation == oldrelation or relation:
            newrelations.append((ws1, relation, ws2))
        elif i == 0 and len(oldrelations) > 1:
            # If the first item is removed, then copy the spacing to the next
            # item
            oldrelations[1] = (ws1, oldrelations[1][1], ws2)

    if all_actions:
        if _relations_empty(newrelations):
            del base[field]
        else:
            base[field] = format_relations(newrelations)
        return all_actions
    return []


def update_conflicts(
        base: Deb822Dict, field: str, checker: PackageChecker) -> List[Action]:
    return filter_relations(
        base, field,
        lambda oldrelation: drop_obsolete_conflicts(checker, oldrelation))


def drop_old_source_relations(
        source, compat_release,
        *, keep_minimum_depends_versions: bool = False
        ) -> List[Tuple[str, List[Action], str]]:
    checker = PackageChecker(compat_release, build=True)
    ret = []
    for field in [
        "Build-Depends",
        "Build-Depends-Indep",
        "Build-Depends-Arch",
    ]:
        actions = update_depends(
            source, field, checker,
            keep_minimum_versions=keep_minimum_depends_versions)
        if actions:
            ret.append((field, actions, compat_release))
    for field in ["Build-Conflicts", "Build-Conflicts-Indep",
                  "Build-Conflicts-Arch"]:
        actions = update_conflicts(source, field, checker)
        if actions:
            ret.append((field, actions, compat_release))
    return ret


def drop_old_binary_relations(
        package_checker, binary, upgrade_release: str, *,
        keep_minimum_depends_versions: bool = False
        ) -> List[Tuple[str, List[Action], str]]:
    ret = []
    for field in ["Depends", "Suggests", "Recommends", "Pre-Depends"]:
        actions = update_depends(
            binary, field, package_checker,
            keep_minimum_versions=keep_minimum_depends_versions)
        if actions:
            ret.append((field, actions, upgrade_release))

    for field in ["Conflicts", "Replaces", "Breaks"]:
        actions = update_conflicts(binary, field, package_checker)
        if actions:
            ret.append((field, actions, upgrade_release))

    return ret


def drop_old_relations(
        editor, package_checker, compat_release: str, upgrade_release: str,
        *, keep_minimum_depends_versions: bool = False
        ) -> List[Tuple[Optional[str], List[Tuple[str, List[Action], str]]]]:
    actions: List[
        Tuple[Optional[str], List[Tuple[str, List[Action], str]]]] = []
    source_actions = []
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
        source_actions.extend(
            drop_old_source_relations(
                editor.source, compat_release,
                keep_minimum_depends_versions=keep_minimum_depends_versions))
    if source_actions:
        actions.append((None, source_actions))

    for binary in editor.binaries:
        binary_actions = drop_old_binary_relations(
            package_checker, binary, upgrade_release,
            keep_minimum_depends_versions=keep_minimum_depends_versions)
        if binary_actions:
            actions.append((binary["Package"], binary_actions))

    return actions


def update_maintscripts(
        wt: WorkingTree, subpath: str, checker: PackageChecker, package: str,
        allow_reformatting: bool = False
        ) -> List[
            Tuple[str, List[Tuple[int, str, Version]]]]:
    ret = []
    for entry in os.scandir(wt.abspath(os.path.join(subpath))):
        if not (entry.name == "maintscript"
                or entry.name.endswith(".maintscript")):
            continue
        with MaintscriptEditor(
                entry.path, allow_reformatting=allow_reformatting) as editor:
            def can_drop(p, v):
                compat_version = checker.package_version(p or package)
                return compat_version is not None and compat_version > v
            removed = drop_obsolete_maintscript_entries(editor, can_drop)
            if removed:
                ret.append((os.path.join(subpath, entry.name), removed))
    return ret


def name_list(packages: List[str]) -> str:
    """Format a list of package names for use in prose.

    Args:
      packages: non-empty list of packages to format
    Returns:
      human-readable string
    """
    if not packages:
        raise ValueError(packages)
    std = list(sorted(set(packages)))
    if len(std) == 1:
        return std[0]
    return ", ".join(std[:-1]) + " and " + std[-1]


class ScrubObsoleteResult:

    control_actions: List[
        Tuple[Optional[str], List[Tuple[str, List[Action], str]]]]

    def __init__(self, specific_files, maintscript_removed, control_actions):
        self.specific_files = specific_files
        self.maintscript_removed = maintscript_removed
        self.control_actions = control_actions
        self.changelog_behaviour = None

    def __bool__(self):
        return bool(self.control_actions) or bool(self.maintscript_removed)

    def value(self) -> int:
        value = DEFAULT_VALUE_MULTIARCH_HINT
        for _para, changes in self.control_actions:
            for _field, actions, _release in changes:
                value += len(actions) * 2
        for _path, removed, _release in self.maintscript_removed:
            value += len(removed)
        return value

    def itemized(self) -> Dict[str, List[str]]:
        summary: Dict[str, List[str]] = {}
        for para, changes in self.control_actions:
            for field, actions, release in changes:
                for action in actions:
                    if para:
                        summary.setdefault(release, []).append(
                            f"{para}: {action} in {field}.")
                    else:
                        summary.setdefault(release, []).append(
                            f"{field}: {action}.")
        if self.maintscript_removed:
            total_entries = sum(
                [len(entries)
                    for name, entries, release in self.maintscript_removed])
            summary.setdefault(self.maintscript_removed[0][2], []).append(
                "Remove %d maintscript entries from %d files." % (
                    total_entries, len(self.maintscript_removed))
            )
        return summary


def _scrub_obsolete(
        wt: WorkingTree,
        debian_path: str,
        *,
        compat_release: str,
        upgrade_release: str,
        allow_reformatting: bool = True,
        keep_minimum_depends_versions: bool = False) -> ScrubObsoleteResult:
    specific_files = []
    binary_package_checker = PackageChecker(upgrade_release, build=False)
    control_path = os.path.join(debian_path, "control")
    try:
        with ControlEditor(
                wt.abspath(control_path),
                allow_reformatting=allow_reformatting) as editor:
            package = editor.source["Source"]
            control_actions = drop_old_relations(
                editor, binary_package_checker, compat_release,
                upgrade_release,
                keep_minimum_depends_versions=keep_minimum_depends_versions)
        specific_files.extend(wt.safe_relpath_files(editor.changed_files))
    except FileNotFoundError as exc:
        if wt.has_filename(os.path.join(debian_path, "debcargo.toml")):
            control_actions = []
        else:
            raise NotDebianPackage(wt, debian_path) from exc

    maintscript_removed = []
    for path, removed in update_maintscripts(
            wt, debian_path, binary_package_checker,
            package, allow_reformatting):
        if removed:
            maintscript_removed.append((path, removed, upgrade_release))
            specific_files.append(path)

    return ScrubObsoleteResult(
        specific_files=specific_files,
        control_actions=control_actions,
        maintscript_removed=maintscript_removed,
    )


def release_aliases(name):
    from distro_info import DebianDistroInfo, UbuntuDistroInfo
    ret = []
    debian_distro_info = DebianDistroInfo()
    ubuntu_distro_info = UbuntuDistroInfo()
    FN_ALIAS_MAP = {
        debian_distro_info.stable: 'stable',
        debian_distro_info.old: 'oldstable',
        debian_distro_info.devel: 'unstable',
        ubuntu_distro_info.lts: 'lts',
        ubuntu_distro_info.stable: 'stable'
    }
    for fn, alias in FN_ALIAS_MAP.items():
        if fn() == name:
            ret.append(alias)
    if ret:
        return '(%s)' % ', '.join(ret)
    return ''


def scrub_obsolete(
        wt: WorkingTree, subpath: str,
        *,
        compat_release: str,
        upgrade_release: str,
        update_changelog=None,
        allow_reformatting: bool = False,
        keep_minimum_depends_versions: bool = False,
        transitions: Optional[Dict[str, str]] = None) -> ScrubObsoleteResult:
    """Scrub obsolete entries.
    """

    if control_files_in_root(wt, subpath):
        debian_path = subpath
    else:
        debian_path = os.path.join(subpath, 'debian')

    result = _scrub_obsolete(
        wt, debian_path, compat_release=compat_release,
        upgrade_release=upgrade_release, allow_reformatting=allow_reformatting,
        keep_minimum_depends_versions=keep_minimum_depends_versions)

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
            update_changelog = dch_guess.update_changelog
            _note_changelog_policy(update_changelog, dch_guess.explanation)
            result.changelog_behaviour = dch_guess
        else:
            # Assume we should update changelog
            update_changelog = True
            result.changelog_behaviour = None

    if update_changelog:
        lines = []
        for release, entries in summary.items():
            rev_aliases = release_aliases(release)
            lines.append(
                "Remove constraints unnecessary since %s" % release
                + ((' ' + rev_aliases) if rev_aliases else '') + ':')
            lines.extend(["+ " + line for line in entries])
        add_changelog_entry(wt, changelog_path, lines)
        specific_files.append(changelog_path)

    lines = []
    for release, entries in summary.items():
        rev_aliases = release_aliases(release)
        lines.extend(
            ["Remove constraints unnecessary since %s" % release
             + ((' ' + rev_aliases) if rev_aliases else ''), ""])
        lines.extend(["* " + line for line in entries])
    lines.extend(["", "Changes-By: deb-scrub-obsolete"])

    committer = get_committer(wt)

    with suppress(PointlessCommit):
        wt.commit(
            specific_files=specific_files,
            message="\n".join(lines),
            allow_pointless=False,
            reporter=NullCommitReporter(),
            committer=committer,
        )

    return result


def versions_dict():
    import lintian_brush
    import debmutate
    import debian
    return {
        "lintian-brush": lintian_brush.version_string,
        "debmutate": debmutate.version_string,
        "debian": debian.__version__,
    }


def report_fatal(code: str, description: str) -> None:
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'versions': versions_dict(),
                'description': description}, f)
    logging.fatal('%s', description)


def report_okay(code: str, description: str) -> None:
    if os.environ.get('SVP_API') == '1':
        with open(os.environ['SVP_RESULT'], 'w') as f:
            json.dump({
                'result_code': code,
                'versions': versions_dict(),
                'description': description}, f)
    logging.info('%s', description)


def main():  # noqa: C901
    import argparse
    import breezy  # noqa: E402
    from breezy.errors import NotBranchError

    breezy.initialize()  # type: ignore
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    from breezy.workspace import (
        check_clean_tree,
        WorkspaceDirty,
        )
    from . import (
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
        default=os.environ.get('COMPAT_RELEASE'))
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
        "--keep-minimum-depends-versions",
        action="store_true",
        help="Keep minimum version dependencies, even when unnecessary")
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
        "--debug", help="Describe all considered changes.", action="store_true"
    )

    args = parser.parse_args()

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format='%(message)s')

    try:
        wt, subpath = WorkingTree.open_containing(args.directory)
    except NotBranchError:
        logging.error(
            "No version control directory found (e.g. a .git directory).")
        return 1

    if args.identity:
        logging.info('%s', get_committer(wt))
        return 0

    with wt.lock_write():
        try:
            check_clean_tree(wt, wt.basis_tree(), subpath)
        except WorkspaceDirty:
            logging.info(
                "%s: Please commit pending changes first.", wt.basedir)
            return 1

        import distro_info
        debian_info = distro_info.DebianDistroInfo()
        upgrade_release = debian_info.codename(args.upgrade_release)

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
            compat_release = debian_info.codename('oldstable')

        if upgrade_release != compat_release:
            logging.info(
                "Removing run time constraints unnecessary since %s"
                " and build time constraints unnecessary since %s",
                upgrade_release, compat_release)
        else:
            logging.info(
                "Removing run time and build time constraints unnecessary "
                "since %s", compat_release)

        if allow_reformatting is None:
            allow_reformatting = False

        if is_debcargo_package(wt, subpath):
            report_fatal("nothing-to-do", "Package uses debcargo")
            return 1
        elif not control_file_present(wt, subpath):
            report_fatal(
                "missing-control-file", "Unable to find debian/control")
            return 1

        try:
            result = scrub_obsolete(
                wt, subpath, compat_release=compat_release,
                upgrade_release=upgrade_release,
                update_changelog=args.update_changelog,
                allow_reformatting=allow_reformatting,
                keep_minimum_depends_versions=(
                    args.keep_minimum_depends_versions)
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
            report_fatal(
                'change-conflict', 'Generated file changes conflict: %s' % e)
            return 1
        except UddTimeout:
            report_fatal('udd-timeout', 'Timeout communicating with UDD')
            return 1

    if not result:
        report_okay("nothing-to-do", "no obsolete constraints")
        return 0

    debian_context = {}
    if result.changelog_behaviour:
        debian_context['changelog'] = result.changelog_behaviour.json()

    if os.environ.get("SVP_API") == "1":
        with open(os.environ["SVP_RESULT"], "w") as f:
            json.dump({
                "description": "Remove constraints unnecessary since %s."
                % upgrade_release,
                "value": result.value(),
                "debian": debian_context,
                "versions": versions_dict(),
                "context": {
                    "specific_files": result.specific_files,
                    "maintscript_removed": [
                        (name, [
                            (lineno, pkg, str(version) if version else None)
                            for (lineno, pkg, version) in entries], release)
                        for (name, entries, release)
                        in result.maintscript_removed],
                    "control_actions": [
                        (pkg, [(field, [action.json() for action in actions],
                                release)
                               for (field, actions, release) in changes])
                        for (pkg, changes) in result.control_actions],
                }
            }, f)

    logging.info("Scrub obsolete settings.")
    for lines in result.itemized().values():
        for line in lines:
            logging.info("* %s", line)

    return 0


if __name__ == "__main__":
    import sys

    sys.exit(main())
