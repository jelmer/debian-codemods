#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import (
    LintianIssue,
    control,
    report_result,
)
from lintian_brush.lintian import LINTIAN_DATA_PATH


def parse(t):
    return tuple([int(v) for v in t.split(".")])


PYTHON_VERSIONS_PATH = os.path.join(LINTIAN_DATA_PATH, "python/versions")


python_versions = {}
try:
    with open(os.path.join(PYTHON_VERSIONS_PATH)) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            key, value = line.split("=")
            python_versions[key.strip()] = parse(value.strip())
except FileNotFoundError:
    sys.exit(2)


with control as updater:
    # Remove anything that involves python 2.6, 2.7, 3.3
    if "X-Python-Version" in updater.source and updater.source[
        "X-Python-Version"
    ].strip().startswith(">= 2."):
        vers = updater.source["X-Python-Version"].split(">=")[1].strip()
        if parse(vers) <= python_versions["old-python2"]:
            if parse(vers) <= python_versions["ancient-python2"]:
                kind = "ancient"
            else:
                kind = "old"
        issue = LintianIssue(
            updater.source,
            f"{kind}-python-version-field",
            "x-python-version {}".format(updater.source["X-Python-Version"]),
        )
        if issue.should_fix():
            del updater.source["X-Python-Version"]
            issue.report_fixed()
    if "X-Python3-Version" in updater.source and updater.source[
        "X-Python3-Version"
    ].strip().startswith(">="):
        vers = updater.source["X-Python3-Version"].split(">=")[1].strip()
        if parse(vers) <= python_versions["old-python3"]:
            if parse(vers) <= python_versions["ancient-python3"]:
                kind = "ancient"
            else:
                kind = "old"
            issue = LintianIssue(
                updater.source,
                f"{kind}-python-version-field",
                "x-python3-version {}".format(updater.source["X-Python3-Version"]),
            )
            if issue.should_fix():
                del updater.source["X-Python3-Version"]
                issue.report_fixed()


report_result(
    "Remove unnecessary X-Python{,3}-Version field in debian/control."
)
