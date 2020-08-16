#!/usr/bin/python3

import os
from lintian_brush.fixer import report_result, fixed_lintian_tag

try:
    with open('debian/tests/control', 'r') as f:
        if f.read().strip() == '':
            os.unlink('debian/tests/control')
            if not os.listdir('debian/tests'):
                os.rmdir('debian/tests')
            fixed_lintian_tag('source', 'empty-debian-tests-control')
except (FileNotFoundError, NotADirectoryError):
    pass

report_result(
    'Remove empty debian/tests/control.')
