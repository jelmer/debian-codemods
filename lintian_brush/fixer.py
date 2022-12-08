#!/usr/bin/python
# Copyright (C) 2019 Jelmer Vernooij
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

"""Helper functions for fixers."""

from collections.abc import MutableMapping
from debmutate.control import ControlEditor
from debmutate.deb822 import Deb822Paragraph
from debmutate.vendor import get_vendor_name
import sys
from typing import Optional, Tuple, Union, List

from . import (
    DEFAULT_MINIMUM_CERTAINTY,
    certainty_sufficient,
)
from .lintian_overrides import (
    get_overrides,
    LintianOverride,
    load_renamed_tags,
)


from debian.changelog import Version
import os


class LintianIssue:
    """Represents a lintian issue."""

    target: Tuple[str, Optional[str]]
    info: Optional[str]
    tag: str

    def __init__(
        self,
        target: Union[Deb822Paragraph, MutableMapping, Tuple[str, str], str],
        tag: str,
        info: Optional[Union[str, Tuple[str, ...]]] = None,
    ):
        if isinstance(target, (Deb822Paragraph, MutableMapping)):
            if "Source" in target:
                self.target = ("source", target["Source"])
            elif "Package" in target:
                self.target = ("binary", target["Package"])
            else:
                raise ValueError(
                    "unable to determine source/binary package from target"
                )
        elif target == "source":
            self.target = ("source", None)
        else:
            self.target = target  # type: ignore
        if isinstance(info, tuple):
            self.info = " ".join(info)
        else:
            self.info = info
        self.tag = tag

    def override_exists(self):
        return _override_exists(
            tag=self.tag, info=self.info, type=self.target[0],
            package=self.target[1]
        )

    def should_fix(self):
        if self.override_exists():
            _overriden_issues.append(self)
            return False
        return True

    def report_fixed(self):
        _fixed_lintian_issues.append(self)

    def __str__(self):
        ret = []
        if self.target[1] is not None:
            ret.append(self.target[1] + " ")
        ret.append(self.target[0])
        ret.append(": " + self.tag + (' ' + self.info) if self.info else '')
        return "".join(ret)

    def __repr__(self):
        return "%s(target=%r, tag=%r, info=%r)" % (
            type(self).__name__,
            self.target,
            self.tag,
            self.info,
        )


_fixed_lintian_issues: List[LintianIssue] = []
_present_overrides: Optional[List[LintianOverride]] = None
_overriden_issues: List[LintianIssue] = []
_tag_renames = None


def _override_exists(
    tag: str,
    info: Optional[str] = None,
    package: Optional[str] = None,
    type: Optional[str] = None,
    arch: Optional[str] = None,
) -> bool:
    global _present_overrides, _tag_renames
    if _present_overrides is None:
        _present_overrides = list(get_overrides())
    if not _present_overrides:
        return False
    if _tag_renames is None:
        _tag_renames = load_renamed_tags()
    for override in _present_overrides:
        if _tag_renames.get(override.tag) == tag:
            tag = override.tag
        if override.matches(
                package=package, info=info, tag=tag, arch=arch, type=type):
            return True
    return False


def fixed_lintian_tags():
    return set([issue.tag for issue in _fixed_lintian_issues])


def fixed_lintian_tag(
    target: Union[MutableMapping, Tuple[str, str], str],
    tag: str,
    info: Optional[Union[str, Tuple[str, ...]]] = None,
):
    """Register a lintian tag as being fixed."""
    LintianIssue(target, tag, info).report_fixed()


def reset() -> None:
    """Reset any global state that may exist."""
    global _fixed_lintian_issues, _present_overrides, _overriden_issues
    _fixed_lintian_issues = []
    _present_overrides = None
    _overriden_issues = []


def report_result(description=None, certainty=None, patch_name=None,
                  details=None):
    """Report the result of a fixer.

    Args:
      description: Description of the fix
      certainty: Certainty of the fix
      patch_name: Suggested patch name, if there are upstream changes
      details: List of details, as lines
    """
    if description:
        print(description)
    if details:
        for detail in details:
            print('+ %s' % detail)
    if certainty:
        print("Certainty: %s" % certainty)
    fixed_lintian_tags = set(
        [issue.tag for issue in _fixed_lintian_issues])
    if fixed_lintian_tags:
        print("Fixed-Lintian-Tags: %s" % ", ".join(sorted(fixed_lintian_tags)))
    if _overriden_issues:
        print("Overridden-Lintian-Issues:")
        for issue in _overriden_issues:
            print(' ' + str(issue))
    if patch_name:
        print("Patch-Name: %s" % patch_name)
    reset()


def net_access_allowed():
    """Check whether network access is allowed."""
    return os.environ.get("NET_ACCESS", "disallow") == "allow"


def compat_release():
    """Codename of oldest release to stay compatible with."""
    return os.environ.get("COMPAT_RELEASE", "sid")


def upgrade_release():
    """Codename of oldest release to allow upgrading from."""
    return os.environ.get("UPGRADE_RELEASE", "oldstable")


def current_package_version():
    return Version(os.environ["CURRENT_VERSION"])


def package_is_native():
    return not current_package_version().debian_revision


def meets_minimum_certainty(certainty):
    return certainty_sufficient(
        certainty,
        os.environ.get("MINIMUM_CERTAINTY", DEFAULT_MINIMUM_CERTAINTY)
    )


def trust_package():
    return os.environ.get("TRUST_PACKAGE") == "true"


def opinionated():
    return os.environ.get("OPINIONATED", "no") == "yes"


def warn(msg):
    sys.stderr.write("%s\n" % msg)


def diligence():
    return int(os.environ.get("DILIGENCE", "0"))


def source_package_name():
    return os.environ.get("DEB_SOURCE")


def is_debcargo_package():
    return os.path.exists('debian/debcargo.toml')


if is_debcargo_package():
    from debmutate.debcargo import DebcargoControlShimEditor
    control = DebcargoControlShimEditor.from_debian_dir('debian')
else:
    control = ControlEditor()


def vendor() -> str:
    return get_vendor_name()


def linenos_to_ranges(linenos):
    ret = []
    cur_range: List[int] = []

    def finalize_range(r):
        if len(r) == 1:
            ret.append("%d" % r[0])
        else:
            ret.append("%d-%d" % (r[0], r[-1]))

    for lineno in linenos:
        if not cur_range or cur_range[-1] == lineno-1:
            cur_range.append(lineno)
        else:
            finalize_range(cur_range)
            cur_range = [lineno]
    if cur_range:
        finalize_range(cur_range)
    return ret


def shorten_path(path):
    ps = path.split('/')
    if ps[0] == 'debian':
        ps[0] = 'd'
    return '/'.join(ps)
