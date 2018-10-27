#!/usr/bin/python3
import os
import sys
if not os.path.exists('debian/source'):
    os.mkdir('debian/source')
if os.path.exists('debian/source/format'):
    # source format file already exists
    sys.exit(0)

from debian.changelog import Changelog
with open('debian/changelog') as f:
  ch = Changelog(f, max_blocks=1)

with open('debian/source/format', 'w') as f:
    if not ch.version.debian_revision:
      print("3.0 (native)")
    else:
      print("3.0 (quilt)")
print("Explicit specify source format.")
print("Fixed-Lintian-Tags: missing-debian-source-format")
