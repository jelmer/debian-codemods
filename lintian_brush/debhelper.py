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
from typing import Dict, Optional, List, Callable

from debian.changelog import Version
from debmutate.control import drop_dependency, add_dependency
from debmutate._rules import update_rules, dh_invoke_drop_with, dh_invoke_add_with

from .lintian import read_debhelper_lintian_data_file, LINTIAN_DATA_PATH


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
    output = subprocess.check_output(["dh_assistant", "which-build-system"]).decode()
    return json.loads(output)["build-system"]


LINTIAN_COMPAT_LEVEL_PATH = os.path.join(LINTIAN_DATA_PATH, "debhelper/compat-level")


def _get_lintian_compat_levels() -> Dict[str, int]:
    with open(LINTIAN_COMPAT_LEVEL_PATH, "r") as f:
        return {
            key: int(value) for (key, value) in read_debhelper_lintian_data_file(f, "=")
        }


def lowest_non_deprecated_compat_level() -> int:
    """Find the lowest non-deprecated debhelper compat level."""
    return _get_lintian_compat_levels()["deprecated"]


def highest_stable_compat_level() -> int:
    """Find the highest stable debhelper compat level."""
    return _get_lintian_compat_levels()["recommended"]


def pedantic_compat_level() -> int:
    """Find the highest stable debhelper compat level."""
    return _get_lintian_compat_levels()["pedantic"]


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
        editor, should_remove: Callable[[str, Version], bool]) -> List[int]:
    """Drop obsolete entries from a maintscript file.

    Args:
      editor: editor to use to access the maintscript
      should_remove: callable to check whether a package/version tuple is obsolete
    Returns:
      list of indexes of entries that were removed
    """
    remove = []
    comments = []
    entries_removed = []
    for i, entry in enumerate(list(editor.lines)):
        if isinstance(entry, str):
            comments.append(i)
            continue
        prior_version = getattr(entry, "prior_version", None)
        if prior_version is not None:
            if should_remove(entry.package, Version(prior_version)):
                remove.extend(comments)
                remove.append(i)
                entries_removed.append(i)
        comments = []
    removed = []
    for i in reversed(remove):
        removed.append(editor.lines[i])
        del editor.lines[i]
    return entries_removed


def drop_sequence(control, rules, sequence):
    new_depends = drop_dependency(
        control.source.get("Build-Depends", ""), "dh-" + sequence)
    if new_depends != control.source['Build-Depends']:
        def drop_with(line, target):
            return dh_invoke_drop_with(line, sequence.replace('-', '_').encode())
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
            return dh_invoke_add_with(line, sequence.replace('-', '_').encode())
        return line
    update_rules(add_with)
