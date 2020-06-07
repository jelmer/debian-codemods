#!/usr/bin/python3

from debmutate.reformatting import check_generated_file
from lintian_brush.fixer import report_result

check_generated_file('debian/rules')

with open('debian/rules', 'rb') as f:
    oldcontents = f.read()

newcontents = oldcontents.replace(b'$(PWD)', b'$(CURDIR)')

if oldcontents != newcontents:
    with open('debian/rules', 'wb') as f:
        f.write(newcontents)

report_result(
    "debian/rules: Avoid using $(PWD) variable.",
    fixed_lintian_tags=['debian-rules-calls-pwd'])
