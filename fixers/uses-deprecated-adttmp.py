#!/usr/bin/python3

from lintian_brush.fixer import report_result
import os
import re
import sys

if not os.path.exists('debian/tests'):
    sys.exit(0)

for entry in os.scandir('debian/tests'):
    if not entry.is_file():
        continue
    newlines = []
    oldlines = []
    with open(entry.path, 'rb') as f:
        oldlines = list(f.readlines())
    newlines = [
        re.sub(b'\\bADTTMP\\b', b'AUTOPKGTEST_TMP', oldline)
        for oldline in oldlines]
    if oldlines != newlines:
        with open(entry.path, 'wb') as f:
            f.writelines(newlines)

report_result(
    'Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.',
    certainty='certain',
    fixed_lintian_tags=['uses-deprecated-adttmp'])
