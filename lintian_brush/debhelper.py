#!/usr/bin/python3
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


"""Debhelper utility functions."""

import os
import subprocess
from typing import Dict, Optional

from .lintian import read_debhelper_lintian_data_file


DEBHELPER_BUILD_STEPS = ['configure', 'build', 'test', 'install', 'clean']


def detect_debhelper_buildsystem(step: Optional[str] = None) -> Optional[str]:
    """Detect the build system for debhelper

    Args:
      step: Optional step to determine the buildsystem for
    Returns:
      Build system name or None, if none could be found
    """
    if os.path.exists('configure.ac') or os.path.exists('configure.in'):
        return 'autoconf'
    output = subprocess.check_output([
        'perl', '-w', '-MDebian::Debhelper::Dh_Lib',
        '-MDebian::Debhelper::Dh_Buildsystems', '-e',
        """\
Debian::Debhelper::Dh_Lib::init();
my $b=Debian::Debhelper::Dh_Buildsystems::load_buildsystem(undef, %(step)s);\
if (defined($b)) { print($b->NAME); } else { print("_undefined_"); }\
""" % {"step": ("'%s'" % step) if step is not None else 'undef'}]).decode()
    if output == '_undefined_':
        return None
    return output


LINTIAN_COMPAT_LEVEL_PATH = '/usr/share/lintian/data/debhelper/compat-level'


def _get_lintian_compat_levels() -> Dict[str, int]:
    with open(LINTIAN_COMPAT_LEVEL_PATH, 'r') as f:
        return {
            key: int(value)
            for (key, value) in read_debhelper_lintian_data_file(f, '=')}


def lowest_non_deprecated_compat_level() -> int:
    """Find the lowest non-deprecated debhelper compat level."""
    return _get_lintian_compat_levels()['deprecated']


def highest_stable_compat_level() -> int:
    """Find the highest stable debhelper compat level."""
    return _get_lintian_compat_levels()['recommended']


def pedantic_compat_level() -> int:
    """Find the highest stable debhelper compat level."""
    return _get_lintian_compat_levels()['pedantic']


debhelper_compat_version: Dict[str, int] = {
    # Debian
    'jessie': 9,
    'stretch': 10,
    'sid': 13,
    'bullseye': 12,
    'buster': 12,

    # Ubuntu
    'xenial': 9,
    'bionic': 11,
    'cosmic': 11,
    'disco': 12,
    'eoan': 12,
    }


def maximum_debhelper_compat_version(compat_release: str) -> int:
    """Retrieve the maximum supported debhelper compat version fior a release.

    Args:
      compat_release: A release name (Debian or Ubuntu, currently)
    Returns:
      debhelper compat version
    """
    max_version = debhelper_compat_version.get(compat_release)
    if max_version is None:
        max_version = lowest_non_deprecated_compat_level()
    return max_version
