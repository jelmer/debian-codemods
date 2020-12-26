#!/usr/bin/python3

from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result, LintianIssue


with ControlEditor() as updater:
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
