#!/usr/bin/python3

from debmutate.reformatting import check_generated_file
from lintian_brush.fixer import report_result, LintianIssue
from lintian_brush.line_editor import LineEditor

check_generated_file('debian/rules')

try:
    with LineEditor('debian/rules', 'b') as e:
        for lineno, line in e:
            newline = line.replace(b'$(PWD)', b'$(CURDIR)')
            if newline != line:
                issue = LintianIssue(
                    'source',
                    'debian-rules-calls-pwd', info='line %d' % lineno)
                if issue.should_fix():
                    e[lineno] = newline
                    issue.report_fixed()
except FileNotFoundError:
    pass

report_result("debian/rules: Avoid using $(PWD) variable.")
