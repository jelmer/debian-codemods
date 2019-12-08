#!/usr/bin/python3

import re
from lintian_brush.rules import update_rules, Makefile

architecture_included = False
PATH = '/usr/share/dpkg/architecture.mk'
INCLUDE_LINE = b'include ' + PATH.encode('ascii')

_variables = set()


def variable_defined(var):
    global _variables
    if _variables:
        return var in _variables
    k = {}
    with open(PATH, 'r') as f:
        for l in f:
            if l.strip().startswith('$(foreach '):
                vs = l.strip()[len('$(foreach '):].split(',')
                k[vs[0]] = vs[1]
    for machine in k['machine'].split(' '):
        for var in k['var'].split(' '):
            _variables.add('DEB_%s_%s' % (machine, var))


def is_dpkg_architecture_line(line):
    m = re.match(b'([A-Z_]+)[ \t]*[:?]?=[ \t]*(.*)', line.strip())
    if not m:
        return (False, False)
    variable = m.group(1)
    assignment = m.group(2)
    if not variable_defined(variable.decode()):
        return (False, False)
    m = re.match(b'\\$\\(shell dpkg-architecture -q([A-Z_]+)\\)', assignment)
    if not m or variable != m.group(1):
        return (True, False)
    else:
        return (True, True)


def drop_arch_line(line):
    global architecture_included
    if line.strip() == INCLUDE_LINE:
        architecture_included = True
        return line
    (variable_matches, value_matches) = is_dpkg_architecture_line(line)
    if not variable_matches:
        return line
    if not value_matches:
        # The assignment is different; let's include
        # the mk file *before* this assignment
        if not architecture_included and variable_matches:
            architecture_included = True
            return [INCLUDE_LINE, line]
        else:
            return line
    if not architecture_included:
        architecture_included = True
        return INCLUDE_LINE
    return None


try:
    mf = Makefile.from_path('debian/rules')
except FileNotFoundError:
    pass
else:
    for line in mf.contents:
        if (isinstance(line, bytes) and
                is_dpkg_architecture_line(line) == (True, True)):
            update_rules(global_line_cb=drop_arch_line)
            break
print('Rely on pre-initialized dpkg-architecture variables.')
print('Fixed-Lintian-Tags: debian-rules-sets-dpkg-architecture-variable')
