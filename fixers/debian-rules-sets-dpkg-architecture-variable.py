#!/usr/bin/python3

import re
from lintian_brush.rules import update_rules

architecture_included = False


def drop_arch_line(line):
    global architecture_included
    if line.strip() == b'include /usr/share/dpkg/architecture.mk':
        architecture_included = True
        return line
    m = re.match(
        b'([A-Z_]+)[ \t]*[:?]?=[ \t]*'
        b'\\$\\(shell dpkg-architecture -q([A-Z_]+)\\)',
        line.strip())
    if not m:
        return line
    if not (m.group(1).startswith(b'DEB_HOST_') or
            m.group(1).startswith(b'DEB_BUILD_') or
            m.group(1).startswith(b'DEB_TARGET_')):
        return line
    if m.group(1) != m.group(2):
        return line
    if not architecture_included:
        architecture_included = True
        return b'include /usr/share/dpkg/architecture.mk'
    return line


update_rules(global_line_cb=drop_arch_line)
print('Rely on pre-initialized dpkg-architecture variables.')
print('Fixed-Lintian-Tags: debian-rules-sets-dpkg-architecture-variable')
