#!/usr/bin/python3

import os
import re
import sys

from debmutate.debhelper import (
    read_debhelper_compat_file,
    get_debhelper_compat_level_from_control,
    )
from lintian_brush.fixer import control, report_result, LintianIssue
from debmutate._rules import update_rules

try:
    file_compat_version = read_debhelper_compat_file('debian/compat')
except FileNotFoundError:
    file_compat_version = None


with control:
    control_compat_version = get_debhelper_compat_level_from_control(
        control.source)


if control_compat_version is not None and file_compat_version is not None:
    os.remove('debian/compat')
    compat_version = control_compat_version
elif control_compat_version is not None:
    compat_version = control_compat_version
elif file_compat_version is not None:
    compat_version = file_compat_version
else:
    sys.exit(0)


def drop_explicit_dh_compat(line):
    m = re.match(b'export DH_COMPAT[ \t]*=[ \t]*([0-9]+)', line)
    if m:
        issue = LintianIssue(
            'source',
            'declares-possibly-conflicting-debhelper-compat-versions',
            info='rules=%d compat=%s' % (compat_version, m.group(1)))
        if issue.should_fix():
            issue.report_fixed()
            return []
    return line


update_rules(global_line_cb=drop_explicit_dh_compat)
report_result(
    'Avoid setting debhelper compat version in debian/rules '
    'and debian/compat.')
