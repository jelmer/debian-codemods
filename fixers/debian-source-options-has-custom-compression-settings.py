#!/usr/bin/python3

import os
import sys

from lintian_brush.fixer import report_result

try:
    with open('debian/source/options', 'r') as f:
        oldlines = list(f.readlines())
except FileNotFoundError:
    sys.exit(0)


def drop_prior_comments(lines):
    while lines and lines[-1].strip().startswith('#'):
        lines.pop(-1)


dropped = set()

newlines = []
for line in oldlines:
    if line.lstrip().startswith('#'):
        newlines.append(line)
        continue
    try:
        (key, value) = line.split('=', 1)
    except ValueError:
        newlines.append(line)
    else:
        if key.strip() == 'compression':
            drop_prior_comments(newlines)
            dropped.add("custom source compression")
            continue
        if key.strip() == 'compression-level':
            drop_prior_comments(newlines)
            dropped.add("custom source compression level")
            continue
        newlines.append(line)

if newlines:
    with open('debian/source/options', 'w') as f:
        f.writelines(newlines)
elif dropped:
    os.unlink('debian/source/options')

report_result(
    'Drop %s.' % ', '.join(sorted(dropped)),
    fixed_lintian_tags=[
        'custom-compression-in-debian-source-options'],
    )
