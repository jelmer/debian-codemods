#!/usr/bin/python3
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

"""Utility functions for dealing with lintian overrides files."""

import os
from contextlib import suppress
from functools import partial
from typing import Callable, Iterator, List, Optional, Tuple, Union

from debian.changelog import Version
from debmutate.lintian_overrides import (
    LintianOverride,
    LintianOverridesEditor,
    iter_overrides,
)

from . import open_binary


def overrides_paths() -> Iterator[str]:
    for path in ["debian/source/lintian-overrides"]:
        if os.path.exists(path):
            yield path
    if os.path.exists("debian"):
        for entry in os.scandir("debian"):
            if entry.name.endswith(".lintian-overrides"):
                yield entry.path


def update_overrides(
    cb: Callable[[str, int, LintianOverride], Optional[LintianOverride]],
) -> None:
    """Call update_overrides_file on all overrides files.

    Args:
      cb: Callback that modifies overrides; called with path, linenumber and
         an Override object
    """
    for path in overrides_paths():
        update_overrides_file(partial(cb, path), path=path)


def update_overrides_file(
    cb: Callable[[int, LintianOverride], Optional[LintianOverride]],
    path: str = "debian/source/lintian-overrides",
) -> bool:
    """Modify the overrides in a file.

    Args:
      cb: Callback that modifies overrides; called with an Override object
        Should return new override or None to delete override.

    Returns:
        Whether the file was modified
    """
    with LintianOverridesEditor(path=path) as editor:
        new_lines: List[Union[str, LintianOverride]] = []
        comments: List[str] = []
        for lineno, entry in enumerate(editor.lines, 1):
            if isinstance(entry, LintianOverride):
                entry = cb(lineno, entry)
                if entry is not None:
                    new_lines.extend(comments)
                    new_lines.append(entry)
                comments = []
            else:
                comments.append(entry)
        if editor.lines != new_lines + comments:
            editor._parsed = new_lines + comments
            if editor._parsed == []:
                editor._parsed = None
        return editor.has_changed()


def get_overrides(
    type: Optional[str] = None, package: Optional[str] = None
) -> Iterator[LintianOverride]:
    paths = []
    if type in ("source", None):
        paths.extend(
            [
                "debian/source/lintian-overrides",
                "debian/source.lintian-overrides",
            ]
        )
    if type in ("binary", None):
        if package is not None:
            paths.extend([f"debian/{package}.lintian-overrides"])
        elif os.path.isdir("debian"):
            paths.extend(
                [
                    e.path
                    for e in os.scandir("debian")
                    if e.name.endswith(".lintian-overrides")
                ]
            )

    for path in paths:
        with suppress(FileNotFoundError), open(path) as f:
            yield from iter_overrides(f)


def override_exists(
    tag: str,
    info: Optional[str] = None,
    package: Optional[str] = None,
    type: Optional[str] = None,
    arch: Optional[str] = None,
) -> bool:
    """Check if a particular override exists.

    Args:
      tag: Tag name
      info: Optional info
      package: Package
      type: package type (source, binary)
      arch: Architecture
    """
    return any(
        override.matches(
            package=package, info=info, tag=tag, arch=arch, type=type
        )
        for override in get_overrides(type=type, package=package)
    )


def get_unused_overrides(
    packages: List[Tuple[str, str]],
) -> List[Tuple[str, str, Version, str]]:
    from .udd import connect_udd_mirror

    args: List[str] = []
    extra = []
    for type, name in packages:
        extra.append(
            f"package = ${len(args) + 1} AND package_type = ${len(args) + 2}"
        )
        args.extend([name, type])

    udd = connect_udd_mirror()
    with udd.cursor() as cursor:
        cursor.execute(
            """\
select package, package_type, package_version, information
from lintian where tag = 'unused-override' AND ({})
""".format(" OR ".join(extra), *args)
        )
        return list(cursor)


unused_overrides = None


def remove_unused(
    control_paragraphs, ignore_tags=None
) -> List[LintianOverride]:
    if ignore_tags is None:
        ignore_tags = set()
    packages = []
    for para in control_paragraphs:
        if "Source" in para:
            packages.append(("source", para["Source"]))
        else:
            packages.append(("binary", para["Package"]))
    global unused_overrides
    unused_overrides = None
    removed = []

    def is_unused(override, unused_overrides):
        for unused_override in unused_overrides:
            if override.package not in (None, unused_override[0]):
                continue
            if override.type not in (None, unused_override[1]):
                continue
            if override.info:
                expected_info = f"{override.tag} {override.info}"
            else:
                expected_info = override.tag
            if expected_info != unused_override[3]:
                continue
            return True
        return False

    def drop_override(path, lineno, override):
        global unused_overrides
        if unused_overrides is None:
            unused_overrides = get_unused_overrides(packages)
        if (
            is_unused(override, unused_overrides)
            and override.tag not in ignore_tags
        ):
            removed.append(override)
            return None
        return override

    update_overrides(drop_override)
    return removed


def load_renamed_tags():
    import json

    with open_binary("renamed-tags.json") as f:
        return json.load(f)


# INFO_FIXERS and fix_override_info have been migrated to Rust
# See lintian-brush/src/lintian_overrides.rs


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--remove-unused", action="store_true", help="Remove unused overrides."
    )
    args = parser.parse_args()
    if args.remove_unused:
        from debian.deb822 import Deb822

        with open("debian/control") as f:
            removed = remove_unused(Deb822.iter_paragraphs(f))
        print(f"Removed {len(removed)} unused overrides")
    else:
        parser.print_usage()
