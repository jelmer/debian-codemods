#!/usr/bin/python3
from lintian_brush.fixer import LintianIssue, control, report_result

with control as updater:
    for key in list(updater.source):
        if not key.startswith("XS-Vcs-"):
            continue
        issue = LintianIssue(
            updater.source, "adopted-extended-field", info=key
        )
        if not issue.should_fix():
            continue
        updater.source[key[3:]] = updater.source[key]
        del updater.source[key]
        issue.report_fixed()


report_result(
    "Remove unnecessary XS- prefix for Vcs- fields in debian/control."
)
