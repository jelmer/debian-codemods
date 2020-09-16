#!/usr/bin/python3

from lintian_brush.fixer import (
    package_is_native,
    report_result,
    fixed_lintian_tag,
    )
import os
import sys

if not package_is_native():
    # Nothing to do
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
