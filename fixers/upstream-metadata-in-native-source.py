#!/usr/bin/python3

from debian.changelog import Version
from lintian_brush.fixer import report_result
import os
import sys

version = Version(os.environ['CURRENT_VERSION'])
if version.debian_revision:
    # Nothing to do
    sys.exit(0)

try:
    os.unlink('debian/upstream/metadata')
except FileNotFoundError:
    sys.exit(0)

if os.listdir('debian/upstream') == []:
    os.rmdir('debian/upstream')

report_result(
    'Remove debian/upstream/metadata in native source package.',
    fixed_lintian_tags=['upstream-metadata-in-native-source'],
    certainty='certain')
