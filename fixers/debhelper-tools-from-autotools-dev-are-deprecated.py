#!/usr/bin/python3

from debmutate._rules import (
    dh_invoke_drop_with,
    update_rules,
)
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
)

from lintian_brush.fixer import (
    LintianIssue,
    control,
    report_result,
)


def cb(line, target):
    if line.strip() == b"dh_autotools-dev_updateconfig":
        issue = LintianIssue(
            "source",
            "debhelper-tools-from-autotools-dev-are-deprecated",
            info="dh_autotools-dev_updateconfig",
        )
        if issue.should_fix():
            issue.report_fixed()
            return []
    if line.strip() == b"dh_autotools-dev_restoreconfig":
        issue = LintianIssue(
            "source",
            "debhelper-tools-from-autotools-dev-are-deprecated",
            info="dh_autotools-dev_restoreconfig",
        )
        if issue.should_fix():
            issue.report_fixed()
            return []
    newline = dh_invoke_drop_with(line, b"autotools-dev")
    if newline != line:
        issue = LintianIssue(
            "source",
            "debhelper-tools-from-autotools-dev-are-deprecated",
            info="dh ... --with autotools-dev",
        )
        if issue.should_fix():
            line = newline
            issue.report_fixed()
    newline = dh_invoke_drop_with(line, b"autotools_dev")
    if newline != line:
        issue = LintianIssue(
            "source",
            "debhelper-tools-from-autotools-dev-are-deprecated",
            info="dh ... --with autotools_dev",
        )
        if issue.should_fix():
            line = newline
            issue.report_fixed()
    return line


if update_rules(cb):
    with control as updater:
        ensure_minimum_debhelper_version(updater.source, "9.20160114")


report_result("Drop use of autotools-dev debhelper.")
