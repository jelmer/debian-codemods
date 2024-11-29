#!/usr/bin/python3

import os
import re
import sys

from lintian_brush.fixer import LintianIssue, report_result
from lintian_brush.line_editor import LineEditor

if not os.path.isdir("debian/tests"):
    sys.exit(0)

for entry in os.scandir("debian/tests"):
    if not entry.is_file():
        continue
    with LineEditor(entry.path, "b") as e:
        for lineno, oldline in e:
            newline = re.sub(b"\\bADTTMP\\b", b"AUTOPKGTEST_TMP", oldline)
            if oldline == newline:
                continue
            issue = LintianIssue(
                "source",
                "uses-deprecated-adttmp",
                info=f"{entry.path} (line {lineno})"
            )
            if issue.should_fix():
                e[lineno] = newline
                issue.report_fixed()


report_result(
    "Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.",
    certainty="certain",
)
