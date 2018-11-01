#!/usr/bin/python3
import os
import sys

if not os.path.exists('debian/pyversions'):
    sys.exit(2)

with open('debian/pyversions', 'r') as f:
    pyversions = f.read().strip()

if pyversions.startswith('2.'):
    os.unlink('debian/pyversions')

print("Remove obsolete debian/pyversions.")
print("Fixed-Lintian-Tags: debian-pyversions-is-obsolete")
