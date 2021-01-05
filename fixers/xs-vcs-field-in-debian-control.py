#!/usr/bin/python3
from lintian_brush.fixer import control, report_result, LintianIssue


with control as updater:
    for key in list(updater.source):
        if not key.startswith('XS-Vcs-'):
            continue
        issue = LintianIssue(
            updater.source, 'xs-vcs-field-in-debian-control', info=key)
        if not issue.should_fix():
            continue
        updater.source[key[3:]] = updater.source[key]
        del updater.source[key]
        issue.report_fixed()


report_result(
    "Remove unnecessary XS- prefix for Vcs- fields in debian/control.")
