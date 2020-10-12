#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    LintianIssue,
    report_result,
    )


with ControlEditor() as updater:
    # Remove anything that involves python 2.6, 2.7, 3.3
    if "X-Python-Version" in updater.source:
        if updater.source["X-Python-Version"].strip().startswith(">= 2."):
            issue = LintianIssue(
                updater.source, 'ancient-python-version-field',
                'x-python-version %s' % updater.source['X-Python-Version'])
            if issue.should_fix():
                del updater.source["X-Python-Version"]
                issue.report_fixed()
    if ("X-Python3-Version" in updater.source and
            updater.source["X-Python3-Version"].strip().startswith(">=")):
        vers = updater.source["X-Python3-Version"].split(">=")[1].strip()
        if vers in ("3.0", "3.1", "3.2", "3.3", "3.4"):
            issue = LintianIssue(
                updater.source, 'ancient-python-version-field',
                'x-python3-version %s' % updater.source['X-Python3-Version'])
            if issue.should_fix():
                del updater.source["X-Python3-Version"]
                issue.report_fixed()


report_result(
    "Remove unnecessary X-Python{,3}-Version field in debian/control.")
