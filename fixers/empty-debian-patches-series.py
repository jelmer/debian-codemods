#!/usr/bin/python3

import os
import sys
from contextlib import suppress

from lintian_brush.fixer import (
    opinionated,
    report_result,
)

if not opinionated():
    sys.exit(0)

with suppress(FileNotFoundError), open('debian/patches/series') as f:
    if not f.read().strip():
        os.unlink('debian/patches/series')

report_result('Remove empty debian/patches/series.')
