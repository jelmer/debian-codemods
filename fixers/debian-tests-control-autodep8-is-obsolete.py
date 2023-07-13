#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import LintianIssue, report_result

OLD_PATH = 'debian/tests/control.autodep8'
NEW_PATH = 'debian/tests/control'

if not os.path.exists(OLD_PATH):
    sys.exit(0)


issue = LintianIssue(
    'source', 'debian-tests-control-autodep8-is-obsolete',
    info=OLD_PATH)

if not issue.should_fix():
    sys.exit(0)

if not os.path.exists(NEW_PATH):
    os.rename(OLD_PATH, 'debian/tests/control')
    issue.report_fixed()
    report_result(f"Rename obsolete path {OLD_PATH} to {NEW_PATH}.")
else:
    merge_issue = LintianIssue(
        'source', 'debian-tests-control-and-control-autodep8',
        info=f'{OLD_PATH} {NEW_PATH}')
    if not merge_issue.should_fix():
        sys.exit(0)
    merge_issue.report_fixed()
    issue.report_fixed()
    with open(NEW_PATH, 'ab') as outf:
        outf.write(b'\n')
        with open(OLD_PATH, 'rb') as inf:
            outf.writelines(inf.readlines())
    os.unlink(OLD_PATH)
    report_result(f"Merge {OLD_PATH} into {NEW_PATH}.")
