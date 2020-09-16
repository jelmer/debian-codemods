#!/usr/bin/python3

from lintian_brush.fixer import report_result, fixed_lintian_tag
import os
import re
import sys
from typing import List

if not os.path.exists('debian/tests'):
    sys.exit(0)

for entry in os.scandir('debian/tests'):
    if not entry.is_file():
        continue
    newlines: List[bytes] = []
    oldlines = []
    with open(entry.path, 'rb') as f:
        oldlines = list(f.readlines())
    newlines = []
    for lineno, oldline in enumerate(oldlines, 1):
        newline = re.sub(b'\\bADTTMP\\b', b'AUTOPKGTEST_TMP', oldline)
        if oldline != newline:
            fixed_lintian_tag(
                'source', 'uses-deprecated-adttmp',
                info='%s (line %d)' % (entry.path, lineno))
        newlines.append(newline)
    if oldlines != newlines:
        with open(entry.path, 'wb') as f:
            f.writelines(newlines)

report_result(
    'Replace use of deprecated $ADTTMP with $AUTOPKGTEST_TMP.',
    certainty='certain')
