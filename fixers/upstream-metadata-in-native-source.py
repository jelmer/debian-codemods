#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import (
    fixed_lintian_tag,
    opinionated,
    package_is_native,
    report_result,
)

if not package_is_native():
    # Nothing to do
    sys.exit(0)

if not opinionated():
    sys.exit(0)

try:
    os.unlink('debian/upstream/metadata')
except FileNotFoundError:
    sys.exit(0)
else:
    fixed_lintian_tag('source', 'upstream-metadata-in-native-source', ())

if os.listdir('debian/upstream') == []:
    os.rmdir('debian/upstream')

report_result(
    'Remove debian/upstream/metadata in native source package.',
    certainty='certain')
