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

try:
    from functools import cache
except ImportError:
    # Python < 3.8
    from functools import lru_cache

    def cache(user_function):
        return lru_cache(maxsize=None)(user_function)

import json
import os
import subprocess
from typing import Dict, Optional, List, Callable, Tuple

from debian.changelog import Version
from debmutate.control import drop_dependency, add_dependency
from debmutate._rules import (
    update_rules,
    dh_invoke_drop_with,
    dh_invoke_add_with,
)

DEBHELPER_BUILD_STEPS = ["configure", "build", "test", "install", "clean"]


def detect_debhelper_buildsystem(step: Optional[str] = None) -> Optional[str]:
    """Detect the build system for debhelper

    Args:
      step: Optional step to determine the buildsystem for
    Returns:
      Build system name or None, if none could be found
    """
    if os.path.exists("configure.ac") or os.path.exists("configure.in"):
        return "autoconf"
    env = dict(os.environ)
    # Necessary for debhelper (<= 13.5.2), or it'll write debian/.debhelper
    # files.
    env['DH_NO_ACT'] = '1'
    output = subprocess.check_output(
        ["dh_assistant", "which-build-system"], env=env).decode()
    return json.loads(output)["build-system"]


@cache
def _get_lintian_compat_levels() -> Dict[str, int]:
    output = subprocess.check_output(
        ["dh_assistant", "supported-compat-levels"]).decode()
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


def write_rules_template(
        path: str, buildsystem: Optional[str] = None,
        addons: Optional[List[str]] = None,
        env: Optional[Dict[str, str]] = None) -> None:
    if addons is None:
        addons = []
    dh_args = ["$@"]
    if buildsystem:
        dh_args.append("--buildsystem=%s" % buildsystem)
    for addon in addons:
        dh_args.append("--with=%s" % addon)

    with open(path, "w") as f:
        f.write(
            """\
#!/usr/bin/make -f

"""
        )
        if env:
            for key, value in env.items():
                f.write("export %s := %s\n" % (key, value))
            f.write("\n")

        f.write(
            """\
%:
\tdh """
            + " ".join(dh_args)
            + """
"""
        )
    os.chmod(path, 0o755)


def drop_obsolete_maintscript_entries(
        editor, should_remove: Callable[[str, Version], bool]
        ) -> List[Tuple[int, str, Version]]:
    """Drop obsolete entries from a maintscript file.

    Args:
      editor: editor to use to access the maintscript
      should_remove: callable to check whether a package/version tuple is
        obsolete
    Returns:
      list of tuples with index, package, version of entries that were removed
    """
    remove: List[Tuple[int, str, Version]] = []
    comments = []
    entries_removed = []
    for i, entry in enumerate(list(editor.lines)):
        if isinstance(entry, str):
            comments.append((i, None, None))
            continue
        prior_version = getattr(entry, "prior_version", None)
        if prior_version is not None:
            if should_remove(entry.package, Version(prior_version)):
                remove.extend(comments)
                remove.append((i, entry.package, Version(prior_version)))
                entries_removed.append(i)
        comments = []
    removed = []
    for i, pkg, version in reversed(remove):
        removed.append(editor.lines[i])
        del editor.lines[i]
    return entries_removed


def drop_sequence(control, rules, sequence):
    new_depends = drop_dependency(
        control.source.get("Build-Depends", ""), "dh-" + sequence)
    if new_depends != control.source['Build-Depends']:
        def drop_with(line, target):
            return dh_invoke_drop_with(
                line, sequence.replace('-', '_').encode())
        update_rules(drop_with)
    new_depends = drop_dependency(
        new_depends, "dh-sequence-" + sequence)
    if control['Build-Depends'] == new_depends:
        return False
    control['Build-Depends'] = new_depends
    return True


def add_sequence(control, rules, sequence):
    control.source['Build-Depends'] = add_dependency(
        control.source.get('Build-Depends'), 'dh-vim-addon')

    def add_with(line, target):
        if line.startswith(b'dh ') or line.startswith(b'dh_'):
            return dh_invoke_add_with(
                line, sequence.replace('-', '_').encode())
        return line
    update_rules(add_with)
