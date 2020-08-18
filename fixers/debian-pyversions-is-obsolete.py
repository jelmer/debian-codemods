#!/usr/bin/python3
import os
import sys

from lintian_brush.fixer import report_result, fixed_lintian_tag

if not os.path.exists('debian/pyversions'):
    sys.exit(2)

with open('debian/pyversions', 'r') as f:
    pyversions = f.read().strip()

if pyversions.startswith('2.'):
    os.unlink('debian/pyversions')
    fixed_lintian_tag('source', "debian-pyversions-is-obsolete")

report_result("Remove obsolete debian/pyversions.")
