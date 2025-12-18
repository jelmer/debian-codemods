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

import json
import os
import subprocess
from functools import cache
from typing import Dict, Optional

DEBHELPER_BUILD_STEPS = ["configure", "build", "test", "install", "clean"]


def detect_debhelper_buildsystem(step: Optional[str] = None) -> Optional[str]:
    """Detect the build system for debhelper.

    Args:
      step: Optional step to determine the buildsystem for
    Returns:
      Build system name or None, if none could be found
    """
    if os.path.exists("configure.ac") or os.path.exists("configure.in"):
        return "autoconf"
    env = {"PATH": os.environ["PATH"]}
    # Necessary for debhelper (<= 13.5.2), or it'll write debian/.debhelper
    # files.
    env["DH_NO_ACT"] = "1"
    output = subprocess.check_output(
        ["dh_assistant", "which-build-system"], env=env
    ).decode()
    return json.loads(output)["build-system"]


@cache
def _get_lintian_compat_levels() -> Dict[str, int]:
    # TODO(jelmer): ideally we should be getting these numbers from the
    # compat-release dh_assistant, rather than what's on the system
    output = subprocess.check_output(
        ["dh_assistant", "supported-compat-levels"]
    ).decode()
    return json.loads(output)


def lowest_non_deprecated_compat_level() -> int:
    """Find the lowest non-deprecated debhelper compat level."""
    return _get_lintian_compat_levels()["LOWEST_NON_DEPRECATED_COMPAT_LEVEL"]


def highest_stable_compat_level() -> int:
    """Find the highest stable debhelper compat level."""
    return _get_lintian_compat_levels()["HIGHEST_STABLE_COMPAT_LEVEL"]


def maximum_debhelper_compat_version(compat_release: str) -> int:
    """Retrieve the maximum supported debhelper compat version fior a release.

    Args:
      compat_release: A release name (Debian or Ubuntu, currently)

    Returns:
      debhelper compat version
    """
    from .release_info import key_package_version

    debhelper_version = key_package_version("debhelper", compat_release)
    if debhelper_version is None:
        max_version = lowest_non_deprecated_compat_level()
    else:
        max_version = int(str(debhelper_version).split(".")[0])
    return max_version
