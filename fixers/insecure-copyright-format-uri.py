#!/usr/bin/python3

import re

from lintian_brush.fixer import fixed_lintian_tag, report_result

with open('debian/copyright', 'rb') as f:
    lines = list(f)

m = re.match(
    rb'^(Format|Format-Specification): '
    rb'(http:\/\/www.debian.org\/doc\/packaging-manuals\/'
    rb'copyright-format\/1.0.*)\n', lines[0])
if m:
    lines[0] = (
        b'Format: https://www.debian.org/doc/packaging-manuals/'
        b'copyright-format/1.0/\n')
    with open('debian/copyright', 'wb') as f:
        f.writelines(lines)
    fixed_lintian_tag(
        'source', 'insecure-copyright-format-uri', m.group(2).decode())

report_result("Use secure copyright file specification URI.")
