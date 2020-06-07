#!/usr/bin/python3

import sys
from lintian_brush.control import (
    ensure_minimum_version,
    get_relation,
    read_debhelper_compat_file,
    ControlUpdater,
    )
from lintian_brush.fixer import report_result


# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    minimum_version = read_debhelper_compat_file('debian/compat')
except FileNotFoundError:
    sys.exit(0)

with ControlUpdater() as updater:
    try:
        get_relation(updater.source.get("Build-Depends", ""),
                     "debhelper-compat")
    except KeyError:
        updater.source["Build-Depends"] = ensure_minimum_version(
                updater.source.get("Build-Depends", ""),
                "debhelper",
                "%s~" % minimum_version)


report_result(
    "Bump debhelper dependency to >= %s, since that's what is "
    "used in debian/compat." % minimum_version,
    fixed_lintian_tags=['package-needs-versioned-debhelper-build-depends'])
