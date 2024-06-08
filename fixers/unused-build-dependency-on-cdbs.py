#!/usr/bin/python3

import sys

from debmutate.control import drop_dependency
from lintian_brush.fixer import LintianIssue, control, report_result

try:
    with open("debian/rules", "rb") as f:
        for line in f:
            if b"/usr/share/cdbs/" in line:
                uses_cdbs = True
                break
        else:
            uses_cdbs = False
except FileNotFoundError:
    # Unsure whether it actually needs cdbs
    sys.exit(2)

if not uses_cdbs:
    with control as updater:
        for field in ["Build-Depends", "Build-Depends-Indep"]:
            new_depends = drop_dependency(
                updater.source.get(field, ""), "cdbs"
            )
            if new_depends != updater.source.get(field, ""):
                issue = LintianIssue(
                    updater.source, "unused-build-dependency-on-cdbs", ""
                )
                if issue.should_fix():
                    updater.source[field] = new_depends
                    if not updater.source[field]:
                        del updater.source[field]
                    issue.report_fixed()

report_result("Drop unused build-dependency on cdbs.")
