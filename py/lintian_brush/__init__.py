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
from typing import (
    Optional,
    Sequence,
)

import breezy.bzr  # noqa: F401
import breezy.git  # noqa: F401
from breezy.workingtree import WorkingTree

__version__ = (0, 158)
version_string = ".".join(map(str, __version__))
SUPPORTED_CERTAINTIES = ["certain", "confident", "likely", "possible", None]
DEFAULT_MINIMUM_CERTAINTY = "certain"
USER_AGENT = "lintian-brush/" + version_string
# Too aggressive?
DEFAULT_URLLIB_TIMEOUT = 3
logger = logging.getLogger(__name__)


class UnsupportedCertainty(Exception):
    """Unsupported certainty."""


def min_certainty(certainties: Sequence[str]) -> str:
    return confidence_to_certainty(
        max([SUPPORTED_CERTAINTIES.index(c) for c in certainties] + [0])
    )


def confidence_to_certainty(confidence: Optional[int]) -> str:
    if confidence is None:
        return "unknown"
    try:
        return SUPPORTED_CERTAINTIES[confidence] or "unknown"
    except IndexError as exc:
        raise ValueError(confidence) from exc


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


class NoChanges(Exception):
    """Script didn't make any changes."""

    def __init__(self, fixer, comment=None, overridden_lintian_issues=None):
        super().__init__(fixer, comment)
        self.fixer = fixer
        self.overridden_lintian_issues = overridden_lintian_issues or []


class NotDebianPackage(Exception):
    """The specified directory does not contain a Debian package."""

    def __init__(self, abspath):
        super().__init__(abspath)


def open_binary(name):
    return open(data_file_path(name), "rb")  # noqa: SIM115


def data_file_path(name, check=os.path.exists):
    # There's probably a more Pythonic way of doing this, but
    # I haven't bothered finding out what it is yet..
    path = os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", "..", name)
    )
    if check(path):
        return path

    import pkg_resources

    path = pkg_resources.resource_filename(__name__, f"lintian-brush/{name}")
    if check(path):
        return path

    # Urgh.
    for b in [
        "/usr/share/lintian-brush",
        "/usr/local/share/lintian-brush",
        os.path.join(sys.prefix, "share/lintian-brush"),
    ]:
        path = os.path.join(b, name)
        if check(path):
            return path
    raise RuntimeError(f"unable to find data path: {name}")


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
    def __init__(self, reason):
        super().__init__(reason)


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


def certainty_to_confidence(certainty: Optional[str]) -> Optional[int]:
    if certainty in ("unknown", None):
        return None
    return SUPPORTED_CERTAINTIES.index(certainty)


def is_debcargo_package(tree, subpath):
    return tree.has_filename(os.path.join(tree, "debian", "debcargo.toml"))


def fixable_lintian_tags():
    from ruamel.yaml import YAML
    yaml = YAML()
    with open('fixers/index.desc') as f:
        fixers = yaml.load(f)

    supported_tags = set()
    for fixer in fixers:
        try:
            tags = fixer['lintian-tags']
        except KeyError:
            pass
        else:
            if tags is not None:
                supported_tags.update(tags)

    return supported_tags

