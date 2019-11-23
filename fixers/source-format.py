#!/usr/bin/python3
from debian.changelog import Version
import os
import sys
if not os.path.exists('debian/source/format'):
    format = '1.0'
    tag = 'missing-debian-source-format'
    description = "Explicitly specify source format."
else:
    tag = 'older-source-format'
    description = "Upgrade to newer source format."
    with open('debian/source/format', 'r') as f:
        format = f.read().strip()
        if format != '1.0':
            sys.exit(0)

version = Version(os.environ['CURRENT_VERSION'])

if not os.path.exists('debian/source'):
    os.mkdir('debian/source')

with open('debian/source/format', 'w') as f:
    if not version.debian_revision:
        f.write("3.0 (native)\n")
    else:
        f.write("3.0 (quilt)\n")


print(description)
print("Fixed-Lintian-Tags: %s" % tag)
