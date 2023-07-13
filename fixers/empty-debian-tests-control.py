#!/usr/bin/python3

import os
from contextlib import suppress

from lintian_brush.fixer import LintianIssue, report_result

with suppress(FileNotFoundError, NotADirectoryError), \
        open('debian/tests/control') as f:
    if f.read().strip() == '':
        issue = LintianIssue('source', 'empty-debian-tests-control')
        if issue.should_fix():
            os.unlink('debian/tests/control')
            if not os.listdir('debian/tests'):
                os.rmdir('debian/tests')
            issue.report_fixed()

report_result(
    'Remove empty debian/tests/control.')
