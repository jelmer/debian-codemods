#!/usr/bin/python3

import os
import sys

from lintian_brush.lintian_overrides import override_exists

try:
    patches = set()
    with open('debian/patches/series', 'r') as f:
        for line in f:
            if line.startswith('#'):
                patches.add(line.split('#')[1].strip())
                continue
            line = line.split('#', 1)[0]
            if not line:
                continue
            try:
                patches.add(line.split()[0])
            except IndexError:
                continue
except FileNotFoundError:
    sys.exit(0)

removed = set()

for patch in os.listdir('debian/patches'):
    path = os.path.join('debian', 'patches', patch)
    # Don't delete the series file
    if patch in ('series', '00list'):
        continue
    # Ignore everything that is not a regular file
    if not os.path.isfile(path):
        continue
    # Ignore any README files
    if patch.startswith('README'):
        continue
    # Ignore any notes files
    if patch.startswith('notes'):
        continue
    # Ignore everything that is listed in series
    if patch in patches:
        continue
    if override_exists(
            'patch-file-present-but-not-mentioned-in-series',
            package='source', info=patch):
        continue
    removed.add(patch)
    os.unlink(path)


print("Remove patch%s %s that %s missing from debian/patches/series." %
      ('es' if len(removed) > 1 else '', ', '.join(sorted(removed)),
       'is' if len(removed) == 1 else 'are'))
print("Fixed-Lintian-Tags: patch-file-present-but-not-mentioned-in-series")
