#!/usr/bin/python3

import os
from contextlib import suppress

from lintian_brush.fixer import report_result

with suppress(FileNotFoundError), open("debian/source/options") as f:
    if not f.read().strip():
        os.unlink("debian/source/options")

report_result("Remove empty debian/source/options.")
