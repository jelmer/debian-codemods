#!/usr/bin/python3
import os

from lintian_brush.fixer import report_result, LintianIssue

try:
    st = os.stat('debian/rules')
except FileNotFoundError:
    pass
else:
    if not (st.st_mode & 0o111):
        issue = LintianIssue('source', 'debian-rules-not-executable')
        if issue.should_fix():
            os.chmod('debian/rules', 0o755)
            issue.report_fixed()


report_result('Make debian/rules executable.')
