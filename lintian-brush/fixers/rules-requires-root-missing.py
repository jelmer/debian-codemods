#!/usr/bin/python3

import sys

from debian.changelog import Version
from lintian_brush.fixer import (
    LintianIssue,
    compat_release,
    control,
    is_debcargo_package,
    meets_minimum_certainty,
    report_result,
)
from lintian_brush.release_info import key_package_version

require_root = "no"
CERTAINTY = "possible"

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

if is_debcargo_package():
    sys.exit(2)

oldest_dpkg_version = key_package_version("dpkg", compat_release())

with control as updater:
    current_require_root = updater.source.get("Rules-Requires-Root")
    if not current_require_root and oldest_dpkg_version < Version("1.22.13"):
        issue = LintianIssue(updater.source, "silent-on-rules-requiring-root")
        if issue.should_fix():
            # TODO: add some heuristics to set require_root = "yes" in common
            # cases, like `debian/rules binary` chown(1)'ing stuff
            updater.source["Rules-Requires-Root"] = require_root
            issue.report_fixed()
        report_result(f"Set Rules-Requires-Root: {require_root}.", certainty=CERTAINTY)
    else:
        # The default value is "no" as of dpkg 1.22.13
        # If the oldest support version of dpkg is >= 1.22.13, we can assume
        # that the field can be unset if it is "no".
        if current_require_root == "no" and oldest_dpkg_version >= Version("1.22.13"):
            del updater.source["Rules-Requires-Root"]
            report_result("Removed Rules-Requires-Root.", certainty=CERTAINTY)
