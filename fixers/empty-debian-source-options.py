#!/usr/bin/python3

import os
with open('debian/source/options', 'r') as f:
    if not f.read().strip():
        os.unlink('debian/source/options')

print('Remove empty debian/source/options.')
