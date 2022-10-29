#!/usr/bin/python3

import re

from lintian_brush.fixer import (
    report_result,
    LintianIssue,
)
from lintian_brush.lintian_overrides import (
    update_overrides,
    LintianOverride,
)

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
}

linenos = []


def fix_info(path, lineno, override):
    if not override.info:
        return override
    try:
        fixer = INFO_FIXERS[override.tag]
    except KeyError:
        pass  # no rename
    else:
        if isinstance(fixer, tuple):
            info = re.sub(fixer[0], fixer[1], override.info)
        elif callable(fixer):
            info = fixer(info) or info
        else:
            raise TypeError(fixer)
        if info != override.info:
            linenos.append(lineno)
        issue = LintianIssue(
            (override.type, override.package), 'mismatched-override',
            override.info + '[%s:%d]' % (path, lineno))
        if issue.should_fix():
            issue.report_fixed()
            return LintianOverride(
                package=override.package, archlist=override.archlist,
                type=override.type, tag=override.tag,
                info=info)
    return override


update_overrides(fix_info)

report_result(
    "Update lintian override info to new format on line %s."
    % ', '.join(map(str, linenos)))
