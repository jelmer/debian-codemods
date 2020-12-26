#!/usr/bin/python3

import re

from lintian_brush.fixer import LintianIssue, report_result

with open('debian/copyright', 'rb') as f:
    lines = list(f)

m = re.match(
    rb'^(Format|Format-Specification): '
    rb'(http:\/\/www.debian.org\/doc\/packaging-manuals\/'
    rb'copyright-format\/1.0.*)\n', lines[0])
if m:
    newline = (
        b'Format: https://www.debian.org/doc/packaging-manuals/'
        b'copyright-format/1.0/\n')
    if newline != lines[0]:
        lines[0] = newline
        issue = LintianIssue(
            'source', 'insecure-copyright-format-uri', m.group(2).decode())
        if issue.should_fix():
            with open('debian/copyright', 'wb') as f:
                f.writelines(lines)
            issue.report_fixed()

report_result("Use secure copyright file specification URI.")
