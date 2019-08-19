#!/usr/bin/python3

import sys

try:
    with open('debian/source/options', 'r') as f:
        oldlines = list(f.readlines())
except FileNotFoundError:
    sys.exit(0)

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
            print("Drop custom source compression.")
            continue
        if key.strip() == 'compression-level':
            print("Drop custom source compression level.")
            continue
        newlines.append(line)

with open('debian/source/options', 'w') as f:
    f.writelines(newlines)

print(
    "Fixed-Lintian-Tags: debian-source-options-has-custom-compression-settings"
    )
