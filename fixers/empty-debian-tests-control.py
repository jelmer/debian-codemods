#!/usr/bin/python3

import os
from lintian_brush.fixer import report_result, LintianIssue

try:
    with open('debian/tests/control', 'r') as f:
        if f.read().strip() == '':
            issue = LintianIssue('source', 'empty-debian-tests-control')
            if issue.should_fix():
                os.unlink('debian/tests/control')
                if not os.listdir('debian/tests'):
                    os.rmdir('debian/tests')
                issue.report_fixed()
except (FileNotFoundError, NotADirectoryError):
    pass

report_result(
    'Remove empty debian/tests/control.')
