#!/usr/bin/python3

import re
import sys

from debmutate.debhelper import get_debhelper_compat_level
from lintian_brush.fixer import report_result, fixed_lintian_tag
from lintian_brush.rules import update_rules

compat_version = get_debhelper_compat_level()
if compat_version is None:
    sys.exit(0)


def drop_explicit_dh_compat(line):
    m = re.match(b'export DH_COMPAT[ \t]*=[ \t]*([0-9]+)', line)
    if m:
        fixed_lintian_tag(
            'source',
            'declares-possibly-conflicting-debhelper-compat-versions',
            info='rules=%d compat=%s' % (compat_version, m.group(1)))
        return []
    return line


update_rules(global_line_cb=drop_explicit_dh_compat)
report_result(
    'Avoid setting debhelper compat version in debian/rules '
    'and debian/compat.')
