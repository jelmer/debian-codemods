#!/usr/bin/python3

import os
import re

from debmutate.control import ensure_some_version

from lintian_brush.fixer import (
    LintianIssue,
    control,
    report_result,
)
from lintian_brush.line_editor import LineEditor

resolution = ""


def update_configure_ac(path):
    global resolution
    if not os.path.exists(path):
        return

    with LineEditor(path, "b") as e:
        for lineno, line in e:
            m = re.fullmatch(
                b"\\s*AC_PATH_PROG\\s*"
                b"\\(\\s*(\\[)?(?P<variable>[A-Z_]+)(\\])?\\s*"
                b",\\s*(\\[)?pkg-config(\\])?\\s*"
                b"(,\\s*(\\[)?(?P<default>.*)(\\])?\\s*)?\\)\n",
                line,
            )

            if not m:
                continue

            issue = LintianIssue(
                "source",
                "autotools-pkg-config-macro-not-cross-compilation-" "safe",
                info="%s (line %d)" % (name, lineno),
            )
            if not issue.should_fix():
                continue

            if m.group("variable") == b"PKG_CONFIG" and not m.group("default"):
                e[lineno] = b"PKG_PROG_PKG_CONFIG\n"
                resolution = (
                    "This patch changes it to use "
                    "PKG_PROG_PKG_CONFIG macro from pkg.m4."
                )
                # Build-Depend on pkg-config for pkg.m4
                with control:
                    control.source["Build-Depends"] = ensure_some_version(
                        control.source.get("Build-Depends", ""), "pkg-config"
                    )
            else:
                e[lineno] = line.replace(b"AC_PATH_PROG", b"AC_PATH_TOOL")
                resolution = "This patch changes it to use AC_PATH_TOOL."

            issue.report_fixed()


for name in ["configure.ac", "configure.in"]:
    update_configure_ac(name)


report_result(
    f"""Use cross-build compatible macro for finding pkg-config.

The package uses AC_PATH_PROG to discover the location of pkg-config(1). This
macro fails to select the correct version to support cross-compilation.

{resolution}

Refer to https://bugs.debian.org/884798 for details.
""",
    patch_name="ac-path-pkgconfig",
)
