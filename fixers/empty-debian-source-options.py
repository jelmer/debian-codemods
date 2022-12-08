#!/usr/bin/python3

from contextlib import suppress
from lintian_brush.fixer import report_result

import os

with suppress(FileNotFoundError), open('debian/source/options', 'r') as f:
    if not f.read().strip():
        os.unlink('debian/source/options')

report_result('Remove empty debian/source/options.')
