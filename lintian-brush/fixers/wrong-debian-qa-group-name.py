#!/usr/bin/python3

from email.utils import parseaddr

from lintian_brush.fixer import LintianIssue, control, report_result

QA_MAINTAINER = "Debian QA Group <packages@qa.debian.org>"

with control as updater:
    try:
        fullname, email = parseaddr(updater.source["Maintainer"])
    except KeyError:
        # No maintainer? Weird, but sure.
        pass
    else:
        if email == "packages@qa.debian.org":
            issue = LintianIssue(
                updater.source,
                "faulty-debian-qa-group-phrase",
                f"Maintainer {fullname}",
            )
            if issue.should_fix():
                updater.source["Maintainer"] = QA_MAINTAINER
                issue.report_fixed()


report_result("Fix Debian QA group name.")
