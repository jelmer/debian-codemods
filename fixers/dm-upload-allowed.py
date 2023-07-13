#!/usr/bin/python3

from lintian_brush.fixer import LintianIssue, control, report_result

with control as updater:
    try:
        old = updater.source["DM-Upload-Allowed"]
    except KeyError:
        pass
    else:
        issue = LintianIssue(
            updater.source, 'malformed-dm-upload-allowed',
            info=old)
        if issue.should_fix():
            del updater.source["DM-Upload-Allowed"]
            issue.report_fixed()


report_result(
    "Remove malformed and unnecessary DM-Upload-Allowed field in "
    "debian/control.")
