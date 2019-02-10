#!/usr/bin/python3

from lintian_brush.control import (
    ensure_minimum_version,
    update_control,
    )


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
            control["Build-Depends"],
            "debhelper",
            "%s~" % minimum_version)

# Debian source package is not obliged to contain `debian/compat'.
# Firstly, it may not use debhelper; secondly it may use modern
# `debhelper-compat' dependency style.


try:
    with open('debian/compat', 'r') as f:
        minimum_version = f.read().strip()
except FileNotFoundError:
    minimum_version = 9999  # Not used anyway.
else:
    update_control(source_package_cb=bump_debhelper)

print("Bump debhelper dependency to >= %s, since that's what is "
      "used in debian/compat." % minimum_version)
print("Fixed-Lintian-Tags: package-needs-versioned-debhelper-build-depends")
