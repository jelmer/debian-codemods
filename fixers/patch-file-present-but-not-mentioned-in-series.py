#!/usr/bin/python3

import os
import sys

try:
    with open('debian/patches/series', 'r') as f:
        patches = set([l.strip() for l in f.readlines()])
except FileNotFoundError:
    sys.exit(0)

removed = set()

for patch in os.listdir('debian/patches'):
    path = os.path.join('debian', 'patches', patch)
    # Don't delete the series file
    if patch == 'series':
        continue
    # Ignore everything that is not a regular file
    if not os.path.isfile(path):
        continue
    # Ignore any README files
    if patch.startswith('README'):
        continue
    # Ignore everything that is listed in series
    if patch in patches:
        continue
    removed.add(patch)
    os.unlink(path)


print("Remove patch%s %s that %s missing from debian/patches/series." %
      ('es' if len(removed) > 1 else '', ', '.join(sorted(removed)),
       'is' if len(removed) == 1 else 'are'))
print("Fixed-Lintian-Tags: patch-file-present-but-not-mentioned-in-series")
