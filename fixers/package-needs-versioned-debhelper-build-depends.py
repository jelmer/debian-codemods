#!/usr/bin/python3

import sys
from debmutate.control import (
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    read_debhelper_compat_file,
    )
from lintian_brush.debhelper import (
    pedantic_compat_level,
    )
from lintian_brush.fixer import report_result


tags = []


# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    minimum_version = read_debhelper_compat_file('debian/compat')
except FileNotFoundError:
    sys.exit(0)

with ControlEditor() as updater:
    if ensure_minimum_debhelper_version(
            updater.source, "%s~" % minimum_version):
        tags.append('package-lacks-versioned-build-depends-on-debhelper')
        if minimum_version > pedantic_compat_level():
            tags.append('package-needs-versioned-debhelper-build-depends')


report_result(
    "Bump debhelper dependency to >= %s, since that's what is "
    "used in debian/compat." % minimum_version,
    fixed_lintian_tags=tags)
