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

from debian.changelog import Version
import os
import re
from typing import Optional, Iterator, Callable, List, Tuple

from debmutate.lintian_overrides import (
    LintianOverridesEditor,
    LintianOverride,
    iter_overrides,
)


from . import data_file_path


def overrides_paths() -> Iterator[str]:
    for path in ["debian/source/lintian-overrides"]:
        if os.path.exists(path):
            yield path
    if os.path.exists("debian"):
        for entry in os.scandir("debian"):
            if entry.name.endswith(".lintian-overrides"):
                yield entry.path


def update_overrides(
        cb: Callable[
            [str, int, LintianOverride],
            Optional[LintianOverride]]) -> None:
    """ "Call update_overrides_file on all overrides files.

    Args:
      cb: Callback that modifies overrides; called with path, linenumber and
         an Override object
    """
    for path in overrides_paths():
        update_overrides_file(
            lambda lineno, override: cb(path, lineno, override),
            path=path)


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
        new_lines = []
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
            ["debian/source/lintian-overrides",
             "debian/source.lintian-overrides"]
        )
    if type in ("binary", None):
        if package is not None:
            paths.extend(["debian/%s.lintian-overrides" % package])
        elif os.path.isdir("debian"):
            paths.extend(
                [
                    e.path
                    for e in os.scandir("debian")
                    if e.name.endswith(".lintian-overrides")
                ]
            )

    for path in paths:
        try:
            with open(path, "r") as f:
                yield from iter_overrides(f)
        except FileNotFoundError:
            pass


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
    for override in get_overrides(type=type, package=package):
        if override.matches(
                package=package, info=info, tag=tag, arch=arch, type=type):
            return True
    return False


async def get_unused_overrides(
    packages: List[Tuple[str, str]]
) -> List[Tuple[str, str, Version, str]]:
    from .udd import connect_udd_mirror

    args: List[str] = []
    extra = []
    for (type, name) in packages:
        extra.append(
            "package = $%d AND package_type = $%d" % (
                len(args) + 1, len(args) + 2)
        )
        args.extend([name, type])

    async with await connect_udd_mirror() as udd:
        return list(
            await udd.fetch(
                """\
select package, package_type, package_version, information
from lintian where tag = 'unused-override' AND (%s)
""" % " OR ".join(extra), *args))


unused_overrides = None


def remove_unused(
        control_paragraphs, ignore_tags=None) -> List[LintianOverride]:

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
                expected_info = "%s %s" % (override.tag, override.info)
            else:
                expected_info = override.tag
            if expected_info != unused_override[3]:
                continue
            return True
        return False

    def drop_override(path, lineno, override):
        global unused_overrides
        if unused_overrides is None:
            import asyncio

            loop = asyncio.get_event_loop()
            unused_overrides = loop.run_until_complete(
                get_unused_overrides(packages))
        if (is_unused(override, unused_overrides)
                and override.tag not in ignore_tags):
            removed.append(override)
            return None
        return override

    update_overrides(drop_override)
    return removed


def load_renamed_tags():
    import json
    with open(data_file_path('renamed-tags.json'), "rb") as f:
        return json.load(f)


LINENO_MATCH = r"\d+|\*"

# Common regexes, for convenience:

# "$file (line $lineno)" => "[$file:$lineno]"
PURE_FLN_SUB = (
    r"^(?P<path>[^[].*) \(line (?P<lineno>" + LINENO_MATCH + r")\)$",
    r"[\1:\2]")
# "$file (line $lineno)" => "* [$file:$lineno]"
PURE_FLN_WILDCARD_SUB = (
    r"^(?P<path>.+) \(line (?P<lineno>" + LINENO_MATCH + r")\)$", r"* [\1:\2]")
# "$file" => "[$file]"
PURE_FN_SUB = (r"^(?P<path>[^[].*)", r"[\1]")


# When adding new expressions here, make sure the first argument doesn't match
# on the new format.
INFO_FIXERS = {
    "autotools-pkg-config-macro-not-cross-compilation-safe":
        PURE_FLN_WILDCARD_SUB,
    "debian-rules-parses-dpkg-parsechangelog": PURE_FLN_SUB,
    "debian-rules-should-not-use-custom-compression-settings":
        (r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/rules:\2]"),
    "debian-source-options-has-custom-compression-settings":
        (r"(.*) \(line (" + LINENO_MATCH + r")\)",
            r"\1 [debian/source/options:\2]"),
    "global-files-wildcard-not-first-paragraph-in-dep5-copyright":
        PURE_FLN_SUB,
    "missing-license-paragraph-in-dep5-copyright": (
        r"([^ ]+) (.*) \(line (" + LINENO_MATCH + r")\)",
        r"\2 [\1:\3]"),
    "unused-license-paragraph-in-dep5-copyright": (
        r"([^ ]+) (.*) \(line (" + LINENO_MATCH + r")\)",
        r"\2 [\1:\3]"),
    "license-problem-undefined-license": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/copyright:\2]"),
    "debhelper-tools-from-autotools-dev-are-deprecated": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/rules:\2]"),
    "version-substvar-for-external-package": (
        r"([^ ]+) \(line (" + LINENO_MATCH + r")\) (.*)",
        r"\1 \3 [debian/control:\2]"),
    "debian-watch-uses-insecure-uri": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/watch:\2]"),
    "uses-deprecated-adttmp": (
        r"([^ ]+) \(line (" + LINENO_MATCH + r")\)", r"[\1:\2]"),
    "incomplete-creative-commons-license": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/copyright:\2]"),
    "debian-rules-sets-dpkg-architecture-variable": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/rules:\2]"),
    "override_dh_auto_test-does-not-check-DEB_BUILD_OPTIONS": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/rules:\2]"),
    "dh-quilt-addon-but-quilt-source-format": (
        r"(.*) \(line (" + LINENO_MATCH + r")\)", r"\1 [debian/rules:\2]"),
    "uses-dpkg-database-directly": PURE_FN_SUB,
    "package-contains-documentation-outside-usr-share-doc": PURE_FN_SUB,
    "non-standard-dir-perm": (
        r"^(?P<path>.+) ([0-9]+) \!= ([0-9]+)", r"\2 != \3 [\1]"),
    "non-standard-file-perm": (
        r"^(?P<path>.+) ([0-9]+) \!= ([0-9]+)", r"\2 != \3 [\1]"),
    "executable-is-not-world-readable": (
        r"^(?P<path>.+) ([0-9]+)", r"\1 [\2]"),
    "library-not-linked-against-libc": PURE_FN_SUB,
    "setuid-binary": (
        r"^(?P<path>.+) (?P<mode>[0-9]+) (.+/.+)", r"\2 \3 [\1]"),
    "elevated-privileges": (
        r"^(?P<path>.+) (?P<mode>[0-9]+) (.+/.+)", r"\2 \3 [\1]"),
    "executable-in-usr-lib": PURE_FN_SUB,
    "executable-not-elf-or-script": PURE_FN_SUB,
    "image-file-in-usr-lib": PURE_FN_SUB,
    "extra-license-file": PURE_FN_SUB,
    "script-not-executable": PURE_FN_SUB,
    "shell-script-fails-syntax-check": PURE_FN_SUB,
    "manpage-has-errors-from-man": (r"^(?P<path).+) (.*)", r"\2 [\1]"),
    "groff-message": (r"^(?P<path).+) (.*)", r"\2 [\1]"),
    "source-contains-prebuilt-javascript-object": PURE_FN_SUB,
    "source-contains-prebuilt-java-object": PURE_FN_SUB,
    "source-contains-prebuilt-windows-binary": PURE_FN_SUB,
    "source-contains-prebuilt-doxygen-documentation": PURE_FN_SUB,
    "source-contains-prebuilt-wasm-binary": PURE_FN_SUB,
    "source-contains-prebuilt-binary": PURE_FN_SUB,
    "source-is-missing": PURE_FN_SUB,
    "spelling-error-in-binary": (r"^(?P<path>.+) (.+) (.+)$", r"\2 \3 [\1]"),
    "very-long-line-length-in-source-file":
        (r"(.*) line ([0-9]+) is ([0-9]+) characters long \(>([0-9]+)\)",
         r"\3 > \4 [\1:\2]"),
    "missing-license-text-in-dep5-copyright":
        ("^(?P<path>.+) (.+)$", r"\2 [\1:*\]"),
    "national-encoding": PURE_FN_SUB,
    "no-manual-page": PURE_FN_SUB,
    "package-contains-empty-directory": PURE_FN_SUB,
    "hardening-no-fortify-functions": PURE_FN_SUB,
    "maintainer-manual-page": PURE_FN_SUB,
    "library-package-name-for-application": PURE_FN_SUB,
    "script-with-language-extension": PURE_FN_SUB,
    "license-problem-non-free-img-lenna": PURE_FN_SUB,
    "file-without-copyright-information": ("^(.*)$", r"\1 [debian/copyright]"),
    "globbing-patterns-out-of-order":
        ("^(?P<path>.+) (.+)$", r"\2 [\1:*]"),
    "statically-linked-binary": PURE_FN_SUB,
    "spare-manual-page": PURE_FN_SUB,
    "shared-library-lacks-prerequisites": PURE_FN_SUB,
    "codeless-jar": PURE_FN_SUB,
    "hardening-no-pie": PURE_FN_SUB,
    "obsolete-url-in-packaging": (
        r"^(?P<path>.+) (.+)$", r"\2 [\1]"),
}


def fix_override_info(override):
    try:
        fixers = INFO_FIXERS[override.tag]
    except KeyError:
        # no regex available
        return override.info
    else:
        info = override.info
        if not isinstance(fixers, list):
            fixers = [fixers]
        for fixer in fixers:
            if isinstance(fixer, tuple):
                info = re.sub(fixer[0], fixer[1], override.info)
            elif callable(fixer):
                info = fixer(info) or info
            else:
                raise TypeError(fixer)
        return info


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--remove-unused", action="store_true", help="Remove unused overrides."
    )
    args = parser.parse_args()
    if args.remove_unused:
        from debian.deb822 import Deb822
        with open("debian/control", "r") as f:
            removed = remove_unused(Deb822.iter_paragraphs(f))
        print("Removed %d unused overrides" % len(removed))
    else:
        parser.print_usage()
