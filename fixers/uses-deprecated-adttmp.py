#!/usr/bin/python3

from lintian_brush.fixer import report_result, LintianIssue
from lintian_brush.line_editor import LineEditor
import os
import re
import sys

if not os.path.exists('debian/tests'):
    sys.exit(0)

for entry in os.scandir('debian/tests'):
    if not entry.is_file():
        continue
    with LineEditor(entry.path, 'b') as e:
        for lineno, oldline in e:
            newline = re.sub(b'\\bADTTMP\\b', b'AUTOPKGTEST_TMP', oldline)
            if oldline == newline:
                continue
            issue = LintianIssue(
                'source', 'uses-deprecated-adttmp',
                info='%s (line %d)' % (entry.path, lineno))
            if issue.should_fix():
                e[lineno] = newline
                issue.report_fixed()


report_result(
    'Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.',
    certainty='certain')
