#!/usr/bin/python3
from debian.changelog import Version
import os
import sys
print("Explicitly specify source format.")
if not os.path.exists('debian/source'):
    os.mkdir('debian/source')
if os.path.exists('debian/source/format'):
    # source format file already exists
    sys.exit(0)

version = Version(os.environ['CURRENT_VERSION'])

with open('debian/source/format', 'w') as f:
    if not version.debian_revision:
        f.write("3.0 (native)\n")
    else:
        f.write("3.0 (quilt)\n")
print("Fixed-Lintian-Tags: missing-debian-source-format")
