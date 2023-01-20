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

from contextlib import suppress
from debian.changelog import Version
from functools import partial
import os
import re
from typing import Optional, Iterator, Callable, List, Tuple

from debmutate.lintian_overrides import (
    LintianOverridesEditor,
    LintianOverride,
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
        cb: Callable[
            [str, int, LintianOverride],
            Optional[LintianOverride]]) -> None:
    """ "Call update_overrides_file on all overrides files.

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
        override.matches(package=package, info=info, tag=tag, arch=arch,
                         type=type)
        for override in get_overrides(type=type, package=package))


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
    with open_binary('renamed-tags.json') as f:
        return json.load(f)


LINENO_MATCH = r"(?P<lineno>\d+|\*)"
PATH_MATCH = r"(?P<path>(?!\\\[)[^[ ]+)"

# Common regexes, for convenience:

# "$file (line $lineno)" => "[$file:$lineno]"
PURE_FLN_SUB = (
    r"^" + PATH_MATCH + r" \(line " + LINENO_MATCH + r"\)$",
    r"[\1:\2]")
# "$file (line $lineno)" => "* [$file:$lineno]"
PURE_FLN_WILDCARD_SUB = (
    r"^" + PATH_MATCH + r" \(line " + LINENO_MATCH + r"\)$",
    r"* [\1:\2]")
# "$file (line $lineno) $msg" => "$msg [$file:$lineno]"
INTERTWINED_FLN_SUB = [
    (r"^" + PATH_MATCH + r" (.+) \(line (" + LINENO_MATCH + r"\)",
     r"\2 [\1:\3]"),
    (r"^" + PATH_MATCH + r" (.+) \*", r"\2 [\1:*]"),
    (r"^" + PATH_MATCH + r" \*", r"* [\1:*]"),
]
COPYRIGHT_SUB = [
    (r"^debian/copyright (.+) \(line " + LINENO_MATCH + r"\)",
     r"\1 [debian/copyright:\2]"),
    (r"^debian/copyright (.+) \*", r"\1 [debian/copyright:*]"),
    (r"^debian/copyright \*", r"* [debian/copyright:*]"),
    (r"^([^/ ]+) \*", r"\1 [debian/copyright:*]"),
]
# "$file" => "[$file]"
PURE_FN_SUB = (r"^" + PATH_MATCH + "$", r"[\1]")


# When adding new expressions here, make sure the first argument doesn't match
# on the new format.
INFO_FIXERS = {
    "autotools-pkg-config-macro-not-cross-compilation-safe":
        PURE_FLN_WILDCARD_SUB,
    "debian-rules-parses-dpkg-parsechangelog": PURE_FLN_SUB,
    "debian-rules-should-not-use-custom-compression-settings":
        (r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/rules:\2]"),
    "debian-source-options-has-custom-compression-settings":
        (r"(.*) \(line " + LINENO_MATCH + r"\)",
            r"\1 [debian/source/options:\2]"),
    "global-files-wildcard-not-first-paragraph-in-dep5-copyright":
        PURE_FLN_SUB,
    "missing-license-paragraph-in-dep5-copyright": COPYRIGHT_SUB,
    "missing-license-text-in-dep5-copyright": COPYRIGHT_SUB,
    "unused-license-paragraph-in-dep5-copyright": (
        r"([^ ]+) (.*) \(line " + LINENO_MATCH + r"\)",
        r"\2 [\1:\3]"),
    "license-problem-undefined-license": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/copyright:\2]"),
    "debhelper-tools-from-autotools-dev-are-deprecated": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/rules:\2]"),
    "version-substvar-for-external-package": (
        r"([^ ]+) \(line " + LINENO_MATCH + r"\) (.*)",
        r"\1 \3 [debian/control:\2]"),
    "debian-watch-uses-insecure-uri": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/watch:\2]"),
    "uses-deprecated-adttmp": (
        r"([^ ]+) \(line " + LINENO_MATCH + r"\)", r"[\1:\2]"),
    "incomplete-creative-commons-license": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/copyright:\2]"),
    "debian-rules-sets-dpkg-architecture-variable": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/rules:\2]"),
    "override_dh_auto_test-does-not-check-DEB_BUILD_OPTIONS": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/rules:\2]"),
    "dh-quilt-addon-but-quilt-source-format": (
        r"(.*) \(line " + LINENO_MATCH + r"\)", r"\1 [debian/rules:\2]"),
    "uses-dpkg-database-directly": PURE_FN_SUB,
    "package-contains-documentation-outside-usr-share-doc": PURE_FN_SUB,
    "non-standard-dir-perm": (
        r"^" + PATH_MATCH + r" ([0-9]+) \!= ([0-9]+)", r"\2 != \3 [\1]"),
    "non-standard-file-perm": (
        r"^" + PATH_MATCH + r" ([0-9]+) \!= ([0-9]+)", r"\2 != \3 [\1]"),
    "executable-is-not-world-readable": (
        r"^" + PATH_MATCH + r" ([0-9]+)", r"\2 [\1]"),
    "library-not-linked-against-libc": PURE_FN_SUB,
    "setuid-binary": (
        r"^" + PATH_MATCH + " (?P<mode>[0-9]+) (.+/.+)", r"\2 \3 [\1]"),
    "elevated-privileges": (
        r"^" + PATH_MATCH + " (?P<mode>[0-9]+) (.+/.+)", r"\2 \3 [\1]"),
    "executable-in-usr-lib": PURE_FN_SUB,
    "executable-not-elf-or-script": PURE_FN_SUB,
    "image-file-in-usr-lib": PURE_FN_SUB,
    "extra-license-file": PURE_FN_SUB,
    "script-not-executable": PURE_FN_SUB,
    "shell-script-fails-syntax-check": PURE_FN_SUB,
    "manpage-has-errors-from-man":
        (r"^" + PATH_MATCH + " ([^[]*)", r"\2 [\1]"),
    "groff-message": (
        r"^" + PATH_MATCH + " ([0-9]+): (.+)$", r"\2: \3 [\1:*]"),
    "source-contains-prebuilt-javascript-object": [
        PURE_FN_SUB,
        (r"^(?P<path>[^[ ].+) line length is .*", r"[\1]")],
    "source-contains-prebuilt-java-object": PURE_FN_SUB,
    "source-contains-prebuilt-windows-binary": PURE_FN_SUB,
    "source-contains-prebuilt-doxygen-documentation": PURE_FN_SUB,
    "source-contains-prebuilt-wasm-binary": PURE_FN_SUB,
    "source-contains-prebuilt-binary": PURE_FN_SUB,
    "source-is-missing": [
        (r"^(?P<path>[^[ ].+) line length is .*", r"[\1]"),
        (r"^(?P<path>[^[ ].+) \*", r"[\1]"),
        PURE_FN_SUB],
    "spelling-error-in-binary":
        (r"^" + PATH_MATCH + r" (.+) ([^[/\*]+)$", r"\2 \3 [\1]"),
    "very-long-line-length-in-source-file": [
        (PATH_MATCH +
         r" line ([0-9]+) is ([0-9]+) characters long \(>([0-9]+)\)",
         r"\3 > \4 [\1:\2]"),
        (PATH_MATCH +
         r" line length is ([0-9]+) characters \(>([0-9]+)\)",
         r"\2 > \3 [\1:*]"),
        (r"^" + PATH_MATCH + r" \*", r"* [\1:*]"),
        (r"^" + PATH_MATCH + r" line \*$", r"* [\1:*]")],
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
        ("^(?P<path>.+) ([^[]+)$", r"\2 [\1:*]"),
    "statically-linked-binary": PURE_FN_SUB,
    "spare-manual-page": PURE_FN_SUB,
    "shared-library-lacks-prerequisites": PURE_FN_SUB,
    "codeless-jar": PURE_FN_SUB,
    "hardening-no-pie": PURE_FN_SUB,
    "hardening-no-relro": PURE_FN_SUB,
    "obsolete-url-in-packaging": (
        r"^(?P<path>.+) (.+)$", r"\2 [\1]"),
    "inconsistent-appstream-metadata-license": (
        r'^(.+) \(([^ ]+) \!= ([^)]+)\)$',
        r'\1 (\2 != \3) [debian/copyright]'),
    "source-ships-excluded-file": (
        r"^([^ ]+)$", r"\1 [debian/copyright:*]"),
    "package-installs-java-bytecode": PURE_FN_SUB,
    "jar-not-in-usr-share": PURE_FN_SUB,
    "debconf-is-not-a-registry": ("^" + PATH_MATCH + "$", r"[\1:*]"),
    "unused-debconf-template": ("^([^ ]+)$", r"\1 [*:*]"),
    "apache2-reverse-dependency-calls-invoke-rc.d": (
        "^" + PATH_MATCH + r":([0-9]+)$", r"[\1:\2]"),
    "application-in-library-section": (
        "^([^ ]+) " + PATH_MATCH + "$", r"\1 [\2]"),
    "repeated-path-segment": (
        "^([^ ]+) " + PATH_MATCH + "$", r"\1 [\2]"),
    "symlink-is-self-recursive": (
        "^([^ ]+) " + PATH_MATCH + "$", r"\1 [\2]"),
    "privacy-breach-google-adsense": (
        r"^" + PATH_MATCH + r" \(choke on: ([^\)]+)\)$",
        r"(choke on: \2) [\1]"),
    "systemd-service-file-refers-to-unusual-wantedby-target": (
        "^" + PATH_MATCH + r" ([^[ ]+)$", r"\2 [\1]"),
    "duplicate-font-file": (
        "^" + PATH_MATCH + r" also in ([^[]+)$",
        r"also in (\2) [\1]"),
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
            fixers = [fixers]  # type: ignore
        for fixer in fixers:
            if isinstance(fixer, tuple):
                info = re.sub(fixer[0], fixer[1], info)
                # The regex should only apply once
                if re.sub(fixer[0], fixer[1], info) != info:
                    raise AssertionError(
                        "invalid repeatable regex for {}: {}".format(
                             override.tag, fixer[0]))
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
        with open("debian/control") as f:
            removed = remove_unused(Deb822.iter_paragraphs(f))
        print("Removed %d unused overrides" % len(removed))
    else:
        parser.print_usage()
