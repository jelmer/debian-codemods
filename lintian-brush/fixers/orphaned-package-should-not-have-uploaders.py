#!/usr/bin/python3

from email.utils import parseaddr

from lintian_brush.fixer import LintianIssue, control, report_result

with control as updater:
    if (
        "Maintainer" in updater.source
        and parseaddr(updater.source["Maintainer"])[1]
        == "packages@qa.debian.org"
        and "Uploaders" in updater.source
    ):
        issue = LintianIssue(updater.source, "uploaders-in-orphan", info=())
        if issue.should_fix():
            del updater.source["Uploaders"]
            issue.report_fixed()


report_result("Remove uploaders from orphaned package.")
