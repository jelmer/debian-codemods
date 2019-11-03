#!/usr/bin/python3

import sys

EXPECTED_URL = (
    b'https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/')

try:
    with open('debian/copyright', 'rb') as f:
        lines = list(f.readlines())
    if not lines:
        sys.exit(2)
except FileNotFoundError:
    pass
else:
    import re
    m = re.fullmatch(b'(Format|Format-Specification): (.*)', lines[0].rstrip())
    if m and (m.group(1) != b'Format' or
              m.group(2).rstrip(b'/') != EXPECTED_URL.rstrip(b'/')):
        lines[0] = b'Format: %s\n' % EXPECTED_URL
        with open('debian/copyright', 'wb') as f:
            f.writelines(lines)

print("Use versioned copyright format URI.")
print("Fixed-Lintian-Tags: unversioned-copyright-format-uri")
