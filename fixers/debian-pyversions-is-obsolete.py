#!/usr/bin/python3
import os
import sys

from lintian_brush.fixer import report_result, LintianIssue

if not os.path.exists('debian/pyversions'):
    sys.exit(2)

with open('debian/pyversions') as f:
    pyversions = f.read().strip()

if pyversions.startswith('2.'):
    issue = LintianIssue('source', "debian-pyversions-is-obsolete")
    if issue.should_fix():
        os.unlink('debian/pyversions')
        issue.report_fixed()

report_result("Remove obsolete debian/pyversions.")
