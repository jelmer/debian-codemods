#!/usr/bin/python3

import sys
from lintian_brush.control import (
    ensure_minimum_version,
    get_relation,
    read_debian_compat_file,
    update_control,
    )


def bump_debhelper(control):
    try:
        get_relation(control.get("Build-Depends", ""), "debhelper-compat")
    except KeyError:
        pass
    else:
        return
    control["Build-Depends"] = ensure_minimum_version(
            control.get("Build-Depends", ""),
            "debhelper",
            "%s~" % minimum_version)

# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    minimum_version = read_debian_compat_file('debian/compat')
except FileNotFoundError:
    sys.exit(0)

update_control(source_package_cb=bump_debhelper)

print("Bump debhelper dependency to >= %s, since that's what is "
      "used in debian/compat." % minimum_version)
print("Fixed-Lintian-Tags: package-needs-versioned-debhelper-build-depends")
