#!/usr/bin/python3

import sys

EXPECTED_HEADER = (
    b'Format: '
    b'https://www.debian.org/doc/packaging-manuals/copyright-format/1.0')

try:
    lines = []
    with open('debian/copyright', 'rb') as f:
        line = f.readline()
        if line.rstrip().rstrip(b'/') != EXPECTED_HEADER:
            sys.exit(0)
        lines.append(line)
        for line in f:
            if line.startswith(b'\t'):
                line = b' ' + line[1:]
            lines.append(line)
except FileNotFoundError:
    pass
else:
    with open('debian/copyright', 'wb') as f:
        f.writelines(lines)

print('debian/copyright: Use spaces rather than tabs in continuation lines.')
