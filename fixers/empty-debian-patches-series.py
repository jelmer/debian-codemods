#!/usr/bin/python3

import os
import sys

if os.environ.get('OPINIONATED', 'no') != 'yes':
    sys.exit(0)

try:
    with open('debian/patches/series', 'r') as f:
        if not f.read().strip():
            os.unlink('debian/patches/series')
except FileNotFoundError:
    pass

print('Remove empty debian/patches/series.')
