#!/usr/bin/python3

import re
from typing import Optional, Set, Tuple

from debmutate._rules import Makefile, update_rules
from lintian_brush.fixer import fixed_lintian_tag, report_result

pkg_info_included = False
PATH = "/usr/share/dpkg/pkg-info.mk"
INCLUDE_LINE = b"include " + PATH.encode("ascii")

_KNOWN = [
    (b"dpkg-parsechangelog | sed -n -e 's/^Version: //p'", "DEB_VERSION"),
]

_variables: Set[str] = set()
message = None

var_sre = re.compile(b"([A-Z_]+)[ \t]*([:?]?=)[ \t]*(.*)")


def variable_defined(var):
    global _variables
    if _variables:
        return var in _variables
    with open(PATH, "rb") as f:
        for line in f:
            m = var_sre.match(line.strip())
            if m:
                _variables.add(m.group(1).decode())
    return var in _variables


def is_pkg_info_var_line(
    line: bytes
) -> Tuple[Optional[str], bool, bool, Optional[bool]]:
    m = var_sre.match(line.strip())
    if not m:
        return (None, False, False, None)
    variable = m.group(1).decode()
    hard_assignment = m.group(2) != b"?="
    assignment = m.group(3)
    if not variable_defined(variable):
        return (variable, False, False, hard_assignment)
    m = re.match(b"\\$\\(shell (.*)\\)", assignment)
    if not m:
        return (variable, True, False, hard_assignment)
    cmd = m.group(1)
    for r, v in _KNOWN:
        if r == cmd:
            return (v, True, True, hard_assignment)
    else:
        return (variable, True, False, hard_assignment)


def drop_pkg_info_line(line):
    global pkg_info_included
    if line.strip() == INCLUDE_LINE:
        pkg_info_included = True
        return line
    (variable, variable_matches, value_matches, hard) = is_pkg_info_var_line(
        line
    )
    if not variable_matches:
        return line
    if not value_matches:
        # The assignment is different; let's include
        # the mk file *before* this assignment
        if not pkg_info_included and variable_matches:
            pkg_info_included = True
            return [INCLUDE_LINE, line]
        else:
            return line
    if hard:
        lineno = -1  # TODO(jelmer): Pass this up
        fixed_lintian_tag(
            "source",
            "debian-rules-parses-dpkg-parsechangelog",
            info="%s (line %d)" % (variable, lineno),
        )
    if not pkg_info_included:
        pkg_info_included = True
        return INCLUDE_LINE
    return None


try:
    mf = Makefile.from_path("debian/rules")
except FileNotFoundError:
    pass
else:
    for line in mf.contents:
        if isinstance(line, bytes) and is_pkg_info_var_line(line)[1:3] == (
            True,
            True,
        ):
            update_rules(global_line_cb=drop_pkg_info_line)
            break

report_result("Avoid invoking dpkg-parsechangelog.")
