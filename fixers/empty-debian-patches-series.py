#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import (
    opinionated,
    report_result,
    )


if not opinionated():
    sys.exit(0)

try:
    with open('debian/patches/series', 'r') as f:
        if not f.read().strip():
            os.unlink('debian/patches/series')
except FileNotFoundError:
    pass

report_result('Remove empty debian/patches/series.')
