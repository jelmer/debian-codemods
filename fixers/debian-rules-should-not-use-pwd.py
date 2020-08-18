#!/usr/bin/python3

from debmutate.reformatting import check_generated_file
from lintian_brush.fixer import report_result, fixed_lintian_tag

check_generated_file('debian/rules')

with open('debian/rules', 'rb') as f:
    oldcontents = list(f)

newcontents = []
for lineno, line in enumerate(oldcontents, 1):
    newline = line.replace(b'$(PWD)', b'$(CURDIR)')
    if newline != line:
        fixed_lintian_tag(
            'source', 'debian-rules-calls-pwd', info='line %d' % lineno)
    newcontents.append(newline)

if oldcontents != newcontents:
    with open('debian/rules', 'wb') as f:
        f.writelines(newcontents)

report_result("debian/rules: Avoid using $(PWD) variable.")
