#!/usr/bin/python3

import os
try:
    with open('debian/source/options', 'r') as f:
        if not f.read().strip():
            os.unlink('debian/source/options')
except FileNotFoundError:
    pass

print('Remove empty debian/source/options.')
