#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import report_result

if not os.path.exists('debian/rules'):
    sys.exit(0)

os.chmod('debian/rules', 0o755)

report_result(
    'Mark debian/rules as executable.',
    certainty='certain')
