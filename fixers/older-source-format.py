#!/usr/bin/python3
from debian.changelog import Version
import os
import sys
if not os.path.exists('debian/source/format'):
    # source format file already exists
    sys.exit(0)

with open('debian/source/format', 'r') as f:
    format = f.read().strip()
    if format != '1.0':
        sys.exit(0)

version = Version(os.environ['CURRENT_VERSION'])

with open('debian/source/format', 'w') as f:
    if not version.debian_revision:
        f.write("3.0 (native)\n")
    else:
        f.write("3.0 (quilt)\n")

print("Upgrade to newer source format.")
print("Fixed-Lintian-Tags: older-source-format")
