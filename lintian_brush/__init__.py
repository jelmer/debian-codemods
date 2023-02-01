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

from contextlib import ExitStack
from datetime import datetime
import errno
import io
import itertools
import logging
import os
import re
import subprocess
import sys
import tempfile
import time
import traceback
from typing import (
    Optional,
    List,
    Sequence,
    Iterator,
    Iterable,
    Tuple,
    Union,
    Callable,
)

from debian.changelog import Changelog, Version

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.commit import NullCommitReporter
from breezy.transport import NoSuchFile
from breezy.osutils import is_inside
from breezy.rename_map import RenameMap
from breezy.tree import Tree
from breezy.workingtree import WorkingTree
from breezy.workspace import reset_tree, check_clean_tree


from debmutate.reformatting import FormattingUnpreservable


__version__ = (0, 147)
version_string = ".".join(map(str, __version__))
SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]
DEFAULT_MINIMUM_CERTAINTY = "certain"
USER_AGENT = "lintian-brush/" + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3
logger = logging.getLogger(__name__)


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


class UnsupportedCertainty(Exception):
    """Unsupported certainty."""


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

    def __init__(self, tree, path):
        super().__init__(tree.abspath(path))


class FixerResult:
    """Result of a fixer run."""

    def __init__(
        self,
        description,
        fixed_lintian_tags=None,
        certainty=None,
        patch_name=None,
        revision_id=None,
        fixed_lintian_issues=None,
        overridden_lintian_issues=None,
    ):
        self.description = description
        self.fixed_lintian_issues = fixed_lintian_issues or []
        if fixed_lintian_tags is None:
            fixed_lintian_tags = []
        if fixed_lintian_tags:
            self.fixed_lintian_issues.extend(
                [LintianIssue(tag=tag) for tag in fixed_lintian_tags])
        self.overridden_lintian_issues = overridden_lintian_issues or []
        self.certainty = certainty
        self.patch_name = patch_name
        self.revision_id = revision_id

    @property
    def fixed_lintian_tags(self):
        return [issue.tag for issue in self.fixed_lintian_issues]

    def __repr__(self):
        return (
            "%s(%r, fixed_lintian_issues=%r, "
            "overridden_lintian_issues=%r, certainty=%r, patch_name=%r, "
            "revision_id=%r)"
        ) % (
            self.__class__.__name__,
            self.description,
            self.fixed_lintian_issues,
            self.overridden_lintian_issues,
            self.certainty,
            self.patch_name,
            self.revision_id,
        )

    def __eq__(self, other):
        if not isinstance(other, type(self)):
            return False
        return (
            (self.description == other.description)
            and (self.fixed_lintian_issues == other.fixed_lintian_issues)
            and (self.overridden_lintian_issues
                 == other.overridden_lintian_issues)
            and (self.certainty == other.certainty)
            and (self.patch_name == other.patch_name)
            and (self.revision_id == other.revision_id)
        )


class Fixer:
    """A Fixer script.

    The `lintian_tags` attribute contains the name of the lintian tags this
    fixer addresses.
    """

    def __init__(self, name: str, lintian_tags: Optional[List[str]] = None):
        self.name = name
        self.lintian_tags = lintian_tags or []

    def run(
        self,
        basedir,
        package,
        current_version,
        compat_release,
        minimum_certainty=None,
        trust_package=False,
        allow_reformatting=False,
        net_access=True,
        opinionated=False,
        diligence=0,
    ):
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


class LintianIssue:

    def __init__(
            self,
            tag: str,
            package: Optional[str] = None,
            package_type: Optional[str] = None,
            info: Optional[List[str]] = None):
        self.package = package
        self.package_type = package_type
        self.tag = tag
        self.info = info

    def __eq__(self, other):
        return isinstance(self, type(other)) and (
            self.package == other.package and
            self.package_type == other.package_type and
            self.tag == other.tag and
            self.info == other.info)

    def json(self):
        return {
            "tag": self.tag,
            "info": self.info,
            "package": self.package,
            "package_type": self.package_type,
        }

    @classmethod
    def from_str(cls, text):
        try:
            (before, after) = text.strip().split(':', 1)
        except ValueError:
            package_type = package = None
            after = text
        else:
            try:
                (package_type, package) = before.strip().split(' ')
            except ValueError:
                package = before
                package_type = None
        parts = after.strip().split(' ')
        return cls(
            package=package,
            package_type=package_type,
            tag=parts[0],
            info=parts[1:])


def parse_script_fixer_output(text):
    """Parse the output from a script fixer."""
    description = []
    overridden_issues = []
    fixed_issues = []
    fixed_tags = []
    certainty = None
    patch_name = None
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        # TODO(jelmer): Do this in a slightly less hackish manner
        try:
            (key, value) = lines[i].split(":", 1)
        except ValueError:
            description.append(lines[i])
        else:
            if key == "Fixed-Lintian-Tags":
                fixed_tags.extend(
                    [tag.strip() for tag in value.strip().split(",")])
            elif key == "Fixed-Lintian-Issues":
                i += 1
                while i < len(lines) and lines[i].startswith(' '):
                    fixed_issues.append(LintianIssue.from_str(lines[i][1:]))
                    i += 1
                continue
            elif key == "Overridden-Lintian-Issues":
                i += 1
                while i < len(lines) and lines[i].startswith(' '):
                    overridden_issues.append(
                        LintianIssue.from_str(lines[i][1:]))
                    i += 1
                continue
            elif key == "Certainty":
                certainty = value.strip()
            elif key == "Patch-Name":
                patch_name = value.strip()
            else:
                description.append(lines[i])
        i += 1
    if certainty not in SUPPORTED_CERTAINTIES:
        raise UnsupportedCertainty(certainty)
    return FixerResult(
        "\n".join(description), fixed_tags,
        certainty, patch_name, revision_id=None,
        fixed_lintian_issues=fixed_issues,
        overridden_lintian_issues=overridden_issues)


def determine_env(
    package,
    current_version,
    compat_release,
    minimum_certainty,
    trust_package,
    allow_reformatting,
    net_access,
    opinionated,
    diligence,
):
    env = dict(os.environ.items())
    env["DEB_SOURCE"] = package
    env["CURRENT_VERSION"] = str(current_version)
    env["COMPAT_RELEASE"] = compat_release
    env["MINIMUM_CERTAINTY"] = minimum_certainty
    env["TRUST_PACKAGE"] = "true" if trust_package else "false"
    env["REFORMATTING"] = "allow" if allow_reformatting else "disallow"
    env["NET_ACCESS"] = "allow" if net_access else "disallow"
    env["OPINIONATED"] = "yes" if opinionated else "no"
    env["DILIGENCE"] = str(diligence)
    return env


class PythonScriptFixer(Fixer):
    """A fixer that is implemented as a python script.

    This gets used just for Python scripts, and significantly speeds
    things up because it prevents starting a new Python interpreter
    for every fixer.
    """

    def __init__(self, name, lintian_tags, script_path):
        super().__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return f"<{self.__class__.__name__}({self.name!r})>"

    def __str__(self):
        return self.name

    def run(
        self,
        basedir,
        package,
        current_version,
        compat_release,
        minimum_certainty=DEFAULT_MINIMUM_CERTAINTY,
        trust_package=False,
        allow_reformatting=False,
        net_access=True,
        opinionated=False,
        diligence=0,
    ):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated,
            diligence=diligence,
        )
        try:
            old_env = dict(os.environ)
            old_stderr = sys.stderr
            old_stdout = sys.stdout
            sys.stderr = io.StringIO()
            sys.stdout = io.StringIO()
            os.environ.update(env)
            try:
                old_cwd = os.getcwd()
            except FileNotFoundError:
                old_cwd = None
            try:
                os.chdir(basedir)
                global_vars = {
                    "__file__": self.script_path,
                    "__name__": "__main__",
                }
                with open(self.script_path) as f:
                    code = compile(f.read(), self.script_path, "exec")
                    exec(code, global_vars)
            except FormattingUnpreservable:
                raise
            except SystemExit as e:
                retcode = e.code
            except BaseException as e:
                traceback.print_exception(
                    type(e), e, e.__traceback__, file=sys.stderr)
                raise FixerScriptFailed(
                    self.script_path, 1, sys.stderr.getvalue()) from e
            else:
                retcode = 0
            description = sys.stdout.getvalue()
            err = sys.stderr.getvalue()
        finally:
            os.environ.clear()
            os.environ.update(old_env)
            sys.stderr = old_stderr
            sys.stdout = old_stdout
            if old_cwd is not None:
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
        super().__init__(name, lintian_tags)
        self.script_path = script_path

    def __repr__(self):
        return "<ScriptFixer(%r)>" % self.name

    def __str__(self):
        return self.name

    def run(
        self,
        basedir: str,
        package: str,
        current_version: Version,
        compat_release: str,
        minimum_certainty: str = DEFAULT_MINIMUM_CERTAINTY,
        trust_package: bool = False,
        allow_reformatting: bool = False,
        net_access: bool = True,
        opinionated: bool = False,
        diligence: int = 0,
    ):
        env = determine_env(
            package=package,
            current_version=current_version,
            compat_release=compat_release,
            minimum_certainty=minimum_certainty,
            trust_package=trust_package,
            allow_reformatting=allow_reformatting,
            net_access=net_access,
            opinionated=opinionated,
            diligence=diligence,
        )
        with tempfile.SpooledTemporaryFile() as stderr:
            try:
                p = subprocess.Popen(
                    self.script_path,
                    cwd=basedir,
                    stdout=subprocess.PIPE,
                    stderr=stderr,
                    env=env,
                )
            except OSError as e:
                if e.errno == errno.ENOMEM:
                    raise MemoryError from e
                raise
            (description, err) = p.communicate(b"")
            if p.returncode == 2:
                raise NoChanges(self)
            if p.returncode != 0:
                stderr.seek(0)
                raise FixerScriptFailed(
                    self.script_path,
                    p.returncode,
                    stderr.read().decode("utf-8", "replace"),
                )
        return parse_script_fixer_output(description.decode("utf-8"))


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


def find_fixers_dir() -> str:
    """Find the local directory with lintian fixer scripts."""
    return data_file_path("fixers", os.path.isdir)


def read_desc_file(
        path: str, force_subprocess: bool = False) -> Iterator[Fixer]:
    """Read a description file.

    Args:
      path: Path to read from.
      force_subprocess: Force running as subprocess
    Yields:
      Fixer objects
    """
    from ruamel.yaml import YAML
    yaml = YAML()
    dirname = os.path.dirname(path)
    with open(path) as f:
        data = yaml.load(f)
    for paragraph in data:
        name = os.path.splitext(paragraph["script"])[0]
        script_path = os.path.join(dirname, paragraph["script"])
        tags = paragraph.get("lintian-tags", [])
        if script_path.endswith(".py") and not force_subprocess:
            yield PythonScriptFixer(name, tags, script_path)
        else:
            yield ScriptFixer(name, tags, script_path)


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


def available_lintian_fixers(
    fixers_dir: Optional[str] = None, force_subprocess: bool = False
) -> Iterator[Fixer]:
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
        yield from read_desc_file(
            os.path.join(fixers_dir, n), force_subprocess=force_subprocess)


def increment_version(v: Version) -> None:
    """Increment a version number.

    For native packages, increment the main version number.
    For other packages, increment the debian revision.

    Args:
        v: Version to increment (modified in place)
    """
    if v.debian_revision is not None:
        v.debian_revision = re.sub(
            "\\d+$", lambda x: str(int(x.group()) + 1), v.debian_revision
        )
    else:
        v.upstream_version = re.sub(
            "\\d+$", lambda x: str(int(x.group()) + 1),
            v.upstream_version or ''
        )


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


def only_changes_last_changelog_block(
    tree: WorkingTree, basis_tree: Tree, changelog_path: str, changes
) -> bool:
    """Check whether the only change in a tree is to the last changelog entry.

    Args:
      tree: Tree to analyze
      changelog_path: Path to the changelog file
      changes: Changes in the tree
    Returns:
      boolean
    """
    with tree.lock_read(), basis_tree.lock_read():
        changes_seen = False
        for change in changes:
            if change.path[1] == "":
                continue
            if change.path[1] == changelog_path:
                changes_seen = True
                continue
            if not tree.has_versioned_directories() and is_inside(
                change.path[1], changelog_path
            ):
                continue
            return False
        if not changes_seen:
            return False
        try:
            new_cl = Changelog(tree.get_file_text(changelog_path))
        except NoSuchFile:
            return False
        try:
            old_cl = Changelog(basis_tree.get_file_text(changelog_path))
        except NoSuchFile:
            return True
        if old_cl.distributions != "UNRELEASED":
            return False
        del new_cl._blocks[0]
        del old_cl._blocks[0]
        return str(new_cl) == str(old_cl)


def certainty_sufficient(
    actual_certainty: str, minimum_certainty: Optional[str]
) -> bool:
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


def has_non_debian_changes(changes, subpath):
    for change in changes:
        for path in change.path:
            if path and not is_inside(os.path.join(subpath, "debian"), path):
                return True
    return False


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
        move_upstream_changes_to_patch,
        read_quilt_patches,
        tree_patches_directory,
        PatchSyntax,
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


def run_lintian_fixer(  # noqa: C901
    local_tree: WorkingTree,
    fixer: Fixer,
    committer: Optional[str] = None,
    update_changelog: Union[bool, Callable[[], bool]] = True,
    compat_release: Optional[str] = None,
    minimum_certainty: Optional[str] = None,
    trust_package: bool = False,
    allow_reformatting: bool = False,
    dirty_tracker=None,
    subpath: str = "",
    net_access: bool = True,
    opinionated: Optional[bool] = None,
    diligence: int = 0,
    timestamp: Optional[datetime] = None,
    basis_tree: Optional[Tree] = None,
    changes_by: str = "lintian-brush",
):
    """Run a lintian fixer on a tree.

    Args:
      local_tree: WorkingTree object
      basis_tree: Tree
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
    if basis_tree is None:
        basis_tree = local_tree.basis_tree()

    changelog_path = os.path.join(subpath, "debian/changelog")

    try:
        with local_tree.get_file(changelog_path) as f:
            cl = Changelog(f, max_blocks=1)
    except NoSuchFile as e:
        raise NotDebianPackage(local_tree, subpath) from e
    package = cl.package
    if cl.distributions == "UNRELEASED":
        current_version = cl.version
    else:
        current_version = cl.version
        increment_version(current_version)
    if compat_release is None:
        compat_release = "sid"
    if minimum_certainty is None:
        minimum_certainty = DEFAULT_MINIMUM_CERTAINTY
    logger.debug('Running fixer %r', fixer)
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
            diligence=diligence,
        )
    except BaseException:
        reset_tree(local_tree, basis_tree, subpath,
                   dirty_tracker=dirty_tracker)
        raise
    if not certainty_sufficient(result.certainty, minimum_certainty):
        reset_tree(local_tree, basis_tree, subpath,
                   dirty_tracker=dirty_tracker)
        raise NotCertainEnough(
            fixer, result.certainty, minimum_certainty,
            overridden_lintian_issues=result.overridden_lintian_issues)
    specific_files: Optional[List[str]]
    if dirty_tracker:
        relpaths = dirty_tracker.relpaths()
        # Sort paths so that directories get added before the files they
        # contain (on VCSes where it matters)
        local_tree.add(
            [
                p
                for p in sorted(relpaths)
                if local_tree.has_filename(p) and not local_tree.is_ignored(p)
            ]
        )
        specific_files = [p for p in relpaths if local_tree.is_versioned(p)]
        if not specific_files:
            raise NoChanges(
                fixer, "Script didn't make any changes",
                result.overridden_lintian_issues)
    else:
        local_tree.smart_add([local_tree.abspath(subpath)])
        specific_files = [subpath] if subpath else None

    if local_tree.supports_setting_file_ids():
        RenameMap.guess_renames(basis_tree, local_tree, dry_run=False)

    changes = list(
        local_tree.iter_changes(
            basis_tree,
            specific_files=specific_files,
            want_unversioned=False,
            require_versioned=True,
        )
    )

    if len(local_tree.get_parent_ids()) <= 1 and not changes:
        raise NoChanges(
            fixer, "Script didn't make any changes",
            result.overridden_lintian_issues)

    if not result.description:
        raise DescriptionMissing(fixer)

    lines = result.description.splitlines()
    summary = lines[0]
    details = list(itertools.takewhile(lambda line: line, lines[1:]))

    # If there are upstream changes in a non-native package, perhaps
    # export them to debian/patches
    if (has_non_debian_changes(changes, subpath)
            and current_version.debian_revision):
        try:
            patch_name, specific_files = _upstream_changes_to_patch(
                local_tree,
                basis_tree,
                dirty_tracker,
                subpath,
                result.patch_name or fixer.name,
                result.description,
                timestamp=timestamp,
            )
        except BaseException:
            reset_tree(local_tree, basis_tree, subpath,
                       dirty_tracker=dirty_tracker)
            raise

        summary = f"Add patch {patch_name}: {summary}"

    if only_changes_last_changelog_block(
        local_tree, basis_tree, changelog_path, changes
    ):
        # If the script only changed the last entry in the changelog,
        # don't update the changelog
        update_changelog = False

    if callable(update_changelog):
        update_changelog = update_changelog()

    if update_changelog:
        from .changelog import add_changelog_entry

        add_changelog_entry(local_tree, changelog_path, [summary] + details)
        if specific_files:
            specific_files.append(changelog_path)

    description = result.description + "\n"
    description += "\n"
    description += "Changes-By: %s\n" % changes_by
    for tag in result.fixed_lintian_tags:
        description += "Fixes: lintian: %s\n" % tag
        description += (
            "See-also: https://lintian.debian.org/tags/%s.html\n" % tag)

    if committer is None:
        committer = get_committer(local_tree)

    revid = local_tree.commit(
        description,
        allow_pointless=False,
        reporter=NullCommitReporter(),
        committer=committer,
        specific_files=specific_files,
    )
    result.revision_id = revid
    # TODO(jelmer): Support running sbuild & verify lintian warning is gone?
    return result, summary


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


def get_dirty_tracker(
        local_tree: WorkingTree, subpath: str = "",
        use_inotify: Optional[bool] = None):
    """Create a dirty tracker object."""
    if use_inotify is True:
        from breezy.dirty_tracker import DirtyTracker

        return DirtyTracker(local_tree, subpath)
    elif use_inotify is False:
        return None
    else:
        try:
            from breezy.dirty_tracker import DirtyTracker
        except ImportError:
            return None
        else:
            return DirtyTracker(local_tree, subpath)


def determine_update_changelog(local_tree, debian_path):
    from .detect_gbp_dch import (
        guess_update_changelog,
        ChangelogBehaviour,
        )

    changelog_path = os.path.join(debian_path, 'changelog')

    try:
        with local_tree.get_file(changelog_path) as f:
            cl = Changelog(f)
    except NoSuchFile:
        # If there's no changelog, then there's nothing to update!
        return False

    behaviour = guess_update_changelog(local_tree, debian_path, cl)
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
        # Only Breezy >= 3.3.1 has DirtyTracker as a context manager
        if dirty_tracker and hasattr(dirty_tracker, '__enter__'):
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


def confidence_to_certainty(confidence: Optional[int]) -> str:
    if confidence is None:
        return "unknown"
    try:
        return SUPPORTED_CERTAINTIES[confidence] or "unknown"
    except IndexError as exc:
        raise ValueError(confidence) from exc


def min_certainty(certainties: Sequence[str]) -> str:
    return confidence_to_certainty(
        max([SUPPORTED_CERTAINTIES.index(c) for c in certainties] + [0])
    )


def control_files_in_root(tree: Tree, subpath: str) -> bool:
    debian_path = os.path.join(subpath, "debian")
    if tree.has_filename(debian_path):
        return False
    control_path = os.path.join(subpath, "control")
    if tree.has_filename(control_path):
        return True
    return tree.has_filename(control_path + ".in")


def is_debcargo_package(tree: Tree, subpath: str) -> bool:
    control_path = os.path.join(subpath, "debian", "debcargo.toml")
    return tree.has_filename(control_path)


def control_file_present(tree: Tree, subpath: str) -> bool:
    """Check whether there are any control files present in a tree.

    Args:
      tree: Tree to check
      subpath: subpath to check
    Returns:
      whether control file is present
    """
    for name in ["debian/control", "debian/control.in", "control",
                 "control.in", "debian/debcargo.toml"]:
        name = os.path.join(subpath, name)
        if tree.has_filename(name):
            return True
    return False
