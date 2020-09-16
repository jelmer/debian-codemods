#!/usr/bin/python3

from lintian_brush.fixer import report_result

import os
try:
    with open('debian/source/options', 'r') as f:
        if not f.read().strip():
            os.unlink('debian/source/options')
except FileNotFoundError:
    pass

report_result('Remove empty debian/source/options.')
