#!/usr/bin/python3

import sys
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    read_debhelper_compat_file,
    )
from lintian_brush.debhelper import (
    pedantic_compat_level,
    )
from lintian_brush.fixer import (
    control,
    report_result,
    fixed_lintian_tag,
    )


# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    minimum_version = read_debhelper_compat_file('debian/compat')
except FileNotFoundError:
    sys.exit(0)

with control as updater:
    if ensure_minimum_debhelper_version(
            updater.source, "%s~" % minimum_version):
        fixed_lintian_tag(
            'source',
            'package-lacks-versioned-build-depends-on-debhelper',
            info='%d' % minimum_version)
        if minimum_version > pedantic_compat_level():
            fixed_lintian_tag(
                'source',
                'package-needs-versioned-debhelper-build-depends',
                info='%d' % minimum_version)


report_result(
    "Bump debhelper dependency to >= %s, since that's what is "
    "used in debian/compat." % minimum_version)
