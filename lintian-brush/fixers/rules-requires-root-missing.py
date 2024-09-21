#!/usr/bin/python3

import sys

from lintian_brush.fixer import (
    LintianIssue,
    control,
    is_debcargo_package,
    meets_minimum_certainty,
    report_result,
)

require_root = "no"
CERTAINTY = "possible"

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

if is_debcargo_package():
    sys.exit(2)

with control as updater:
    if "Rules-Requires-Root" not in updater.source:
        issue = LintianIssue(updater.source, "silent-on-rules-requiring-root")
        if issue.should_fix():
            # TODO: add some heuristics to set require_root = "yes" in common
            # cases, like `debian/rules binary` chown(1)'ing stuff
            updater.source["Rules-Requires-Root"] = require_root
            issue.report_fixed()

report_result(
    f"Set Rules-Requires-Root: {require_root}.", certainty=CERTAINTY
)
