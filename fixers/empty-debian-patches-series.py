#!/usr/bin/python3

import os
try:
    with open('debian/patches/series', 'r') as f:
        if not f.read().strip():
            os.unlink('debian/patches/series')
except FileNotFoundError:
    pass

print('Remove empty debian/patches/series.')
