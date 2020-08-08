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

from debmutate.deb822 import Deb822
from typing import Optional, Tuple, Union

from . import (
    DEFAULT_MINIMUM_CERTAINTY,
    certainty_sufficient,
    )


from debian.changelog import Version
import os


_fixed_lintian_tags = []


def fixed_lintian_tag(
        target: Union[Deb822, Tuple[str, str]],
        tag: str, info: Optional[str] = None):
    """Register a lintian tag as being fixed."""
    if isinstance(target, Deb822):
        if 'Source' in target:
            target = ('source', target['Source'])
        elif 'Package' in target:
            target = ('binary', target['Package'])
        else:
            raise ValueError(
                'unable to determine source/binary package from target')
    _fixed_lintian_tags.append((target, tag, info))


def reset() -> None:
    """Reset any global state that may exist."""
    global _fixed_lintian_tags
    _fixed_lintian_tags = []


def report_result(description, fixed_lintian_tags=None, certainty=None,
                  patch_name=None):
    """Report the result of a fixer.

    Args:
      description: Description of the fix
      fixed_lintian_tags: Set of fixed lintian tags
      certainty: Certainty of the fix
      patch_name: Suggested patch name, if there are upstream changes
    """
    print(description)
    if certainty:
        print('Certainty: %s' % certainty)
    fixed_lintian_tags = set(fixed_lintian_tags or [])
    fixed_lintian_tags.update(
        [tag for (target, tag, info) in _fixed_lintian_tags])
    if fixed_lintian_tags:
        print('Fixed-Lintian-Tags: %s' % ', '.join(sorted(fixed_lintian_tags)))
    if patch_name:
        print('Patch-Name: %s' % patch_name)
    reset()


def net_access_allowed():
    """Check whether network access is allowed."""
    return os.environ.get('NET_ACCESS', 'disallow') == 'allow'


def compat_release():
    return os.environ.get('COMPAT_RELEASE', 'sid')


def current_package_version():
    return Version(os.environ['CURRENT_VERSION'])


def package_is_native():
    return (not current_package_version().debian_revision)


def meets_minimum_certainty(certainty):
    return certainty_sufficient(
        certainty,
        os.environ.get('MINIMUM_CERTAINTY', DEFAULT_MINIMUM_CERTAINTY))


def trust_package():
    return os.environ.get('TRUST_PACKAGE') == 'true'


def opinionated():
    return os.environ.get('OPINIONATED', 'no') == 'yes'
