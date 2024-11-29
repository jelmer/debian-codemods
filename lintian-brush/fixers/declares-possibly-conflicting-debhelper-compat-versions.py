#!/usr/bin/python3

import os
import re
import sys
from typing import Optional

from debmutate._rules import update_rules
from debmutate.debhelper import (
    get_debhelper_compat_level_from_control,
    read_debhelper_compat_file,
)

from lintian_brush.fixer import LintianIssue, control, report_result

file_compat_version: Optional[int]
try:
    file_compat_version = read_debhelper_compat_file("debian/compat")
except FileNotFoundError:
    file_compat_version = None


with control:
    control_compat_version = get_debhelper_compat_level_from_control(
        control.source
    )


compat_version: Optional[int]
if control_compat_version is not None and file_compat_version is not None:
    os.remove("debian/compat")
    compat_version = control_compat_version
elif control_compat_version is not None:
    compat_version = control_compat_version
elif file_compat_version is not None:
    compat_version = file_compat_version
else:
    sys.exit(0)


def drop_explicit_dh_compat(line):
    m = re.match(b"export DH_COMPAT[ \t]*=[ \t]*([0-9]+)", line)
    if m:
        rules_version = int(m.group(1).decode("utf-8"))
        if compat_version and compat_version != rules_version:
            issue = LintianIssue(
                "source",
                "declares-possibly-conflicting-debhelper-compat-versions",
                info=f"rules={rules_version} compat={compat_version}",
            )
            if issue.should_fix():
                issue.report_fixed()
                return []
        elif compat_version:
            return []
    return line


update_rules(global_line_cb=drop_explicit_dh_compat)
report_result(
    "Avoid setting debhelper compat version in debian/rules "
    "and debian/compat."
)
