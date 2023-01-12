#!/usr/bin/python3

from contextlib import suppress
import os
from lintian_brush.fixer import report_result, LintianIssue

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
