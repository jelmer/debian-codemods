#!/usr/bin/python3

from debmutate._rules import update_rules
from lintian_brush.fixer import report_result, LintianIssue


def update_line(line, target):
    if line.strip() == b'dh_clean -k':
        issue = LintianIssue('source', "dh-clean-k-is-deprecated", '')
        if issue.should_fix():
            issue.report_fixed()
            return b'dh_prep'
    return line


update_rules(update_line)
report_result("debian/rules: Use dh_prep rather than \"dh_clean -k\".")
