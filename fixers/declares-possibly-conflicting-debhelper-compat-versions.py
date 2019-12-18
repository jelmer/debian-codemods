#!/usr/bin/python3

import re
import sys

from lintian_brush.control import get_debhelper_compat_version
from lintian_brush.rules import update_rules

compat_version = get_debhelper_compat_version()
if compat_version is None:
    sys.exit(0)


def drop_explicit_dh_compat(line):
    if re.match(b'export DH_COMPAT[ \t]*=', line):
        return []
    return line


update_rules(global_line_cb=drop_explicit_dh_compat)
print('Avoid setting debhelper compat version in debian/rules '
      'and debian/compat.')
print('Fixed-Lintian-Tags: '
      'declares-possibly-conflicting-debhelper-compat-versions')
