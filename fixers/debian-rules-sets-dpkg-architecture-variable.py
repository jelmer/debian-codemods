#!/usr/bin/python3

import re
from lintian_brush.rules import update_rules

architecture_included = False
INCLUDE_LINE = b'include /usr/share/dpkg/architecture.mk'


def drop_arch_line(line):
    global architecture_included
    if line.strip() == INCLUDE_LINE:
        architecture_included = True
        return line
    m = re.match(b'([A-Z_]+)[ \t]*[:?]?=[ \t]*(.*)', line.strip())
    if not m:
        return line
    variable = m.group(1)
    assignment = m.group(2)
    if not (variable.startswith(b'DEB_HOST_') or
            variable.startswith(b'DEB_BUILD_') or
            variable.startswith(b'DEB_TARGET_')):
        return line
    m = re.match(b'\\$\\(shell dpkg-architecture -q([A-Z_]+)\\)', assignment)
    if not m or variable != m.group(1):
        # The assignment is different; let's include
        # the mk file *before* this assignment
        if not architecture_included:
            architecture_included = True
            return [INCLUDE_LINE, line]
        else:
            return line
    if not architecture_included:
        architecture_included = True
        return INCLUDE_LINE
    return None


update_rules(global_line_cb=drop_arch_line)
print('Rely on pre-initialized dpkg-architecture variables.')
print('Fixed-Lintian-Tags: debian-rules-sets-dpkg-architecture-variable')
