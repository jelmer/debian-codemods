#!/usr/bin/python3

import sys
from contextlib import suppress

from debmutate.control import ensure_some_version, get_relation
from lintian_brush.fixer import LintianIssue, control, report_result

try:
    from tomlkit import load
except ModuleNotFoundError:
    sys.exit(2)

try:
    with open("pyproject.toml") as f:
        toml = load(f)
except FileNotFoundError:
    sys.exit(0)


build_backend = toml.get("build-system", {}).get("build-backend")

# See /usr/share/lintian/lib/Lintian/Check/Languages/Python.pm
PREREQUISITE_MAP = {
    "poetry.core.masonry.api": "python3-poetry-core",
    "flit_core.buildapi": "flit",
    "setuptools.build_meta": "python3-setuptools",
}


try:
    prerequisite = PREREQUISITE_MAP[build_backend]
except KeyError:
    sys.exit(2)

with control:
    for field in [
        "Build-Depends",
        "Build-Depends-Indep",
        "Build-Depends-Arch",
    ]:
        with suppress(KeyError):
            if get_relation(control.source.get(field, ""), prerequisite):
                sys.exit(0)
    # TOOD(jelmer): Add file:lineno; requires
    # https://github.com/sdispater/tomlkit/issues/55
    issue = LintianIssue(
        control.source,
        "missing-prerequisite-for-pyproject-backend",
        info=f"{build_backend} (does not satisfy {prerequisite})",
    )
    if issue.should_fix():
        control.source["Build-Depends"] = ensure_some_version(
            control.source.get("Build-Depends", ""), prerequisite
        )
        issue.report_fixed()

report_result(
    f"Add missing build-dependency on {prerequisite}.\n\n"
    f"This is necessary for build-backend {build_backend} in pyproject.toml"
)
