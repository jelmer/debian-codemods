#!/usr/bin/python3

import re
from lintian_brush.fixer import report_result, opinionated
from lintian_brush.rules import update_rules, Makefile
from typing import Set, Tuple, Optional

architecture_included = False
PATH = '/usr/share/dpkg/architecture.mk'
INCLUDE_LINE = b'include ' + PATH.encode('ascii')

_variables: Set[str] = set()
fixed_tags = set()
message = None


def variable_defined(var):
    global _variables
    if _variables:
        return var in _variables
    k = {}
    with open(PATH, 'r') as f:
        for line in f:
            if line.strip().startswith('$(foreach '):
                vs = line.strip()[len('$(foreach '):].split(',')
                k[vs[0]] = vs[1]
    for machine in k['machine'].split(' '):
        for var in k['var'].split(' '):
            _variables.add('DEB_%s_%s' % (machine, var))


def is_dpkg_architecture_line(line: str) -> Tuple[bool, bool, Optional[bool]]:
    m = re.match(b'([A-Z_]+)[ \t]*([:?]?=)[ \t]*(.*)', line.strip())
    if not m:
        return (False, False, None)
    variable = m.group(1)
    hard_assignment = m.group(2) != b"?="
    assignment = m.group(3)
    if not variable_defined(variable.decode()):
        return (False, False, hard_assignment)
    m = re.match(b'\\$\\(shell dpkg-architecture -q([A-Z_]+)\\)', assignment)
    if not m or variable != m.group(1):
        return (True, False, hard_assignment)
    else:
        return (True, True, hard_assignment)


def drop_arch_line(line):
    global architecture_included
    if line.strip() == INCLUDE_LINE:
        architecture_included = True
        return line
    (variable_matches, value_matches, hard) = is_dpkg_architecture_line(line)
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
    if hard:
        fixed_tags.add('debian-rules-sets-dpkg-architecture-variable')
    if not architecture_included:
        architecture_included = True
        return INCLUDE_LINE
    return None


def update_assignment_kind(line):
    global architecture_included, message
    if line.strip() == INCLUDE_LINE:
        architecture_included = True
        return line
    (variable_matches, value_matches, hard) = is_dpkg_architecture_line(line)
    if not variable_matches:
        return line
    if not value_matches:
        return line
    if not hard:
        if architecture_included:
            return None
        else:
            # Nothing wrong here
            return line
    fixed_tags.add('debian-rules-sets-dpkg-architecture-variable')
    if architecture_included:
        message = 'Rely on existing architecture.mk include.'
        return None
    return re.sub(b'([:?]?=)', b'?=', line)


if opinionated():
    try:
        mf = Makefile.from_path('debian/rules')
    except FileNotFoundError:
        pass
    else:
        for line in mf.contents:
            if (isinstance(line, bytes) and
                    is_dpkg_architecture_line(line)[:2] == (True, True)):
                update_rules(global_line_cb=drop_arch_line)
                message = (
                    'Rely on pre-initialized dpkg-architecture variables.')
                break
else:
    message = 'Use ?= for assignments to architecture variables.'
    update_rules(global_line_cb=update_assignment_kind)

if message:
    report_result(
        message,
        fixed_lintian_tags=fixed_tags)
