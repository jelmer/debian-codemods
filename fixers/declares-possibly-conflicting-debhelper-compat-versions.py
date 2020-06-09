#!/usr/bin/python3

import re
import sys

from debmutate.debhelper import get_debhelper_compat_level
from lintian_brush.fixer import report_result
from lintian_brush.rules import update_rules

compat_version = get_debhelper_compat_level()
if compat_version is None:
    sys.exit(0)


def drop_explicit_dh_compat(line):
    if re.match(b'export DH_COMPAT[ \t]*=', line):
        return []
    return line


update_rules(global_line_cb=drop_explicit_dh_compat)
report_result(
    'Avoid setting debhelper compat version in debian/rules '
    'and debian/compat.',
    fixed_lintian_tags=[
        'declares-possibly-conflicting-debhelper-compat-versions'])
