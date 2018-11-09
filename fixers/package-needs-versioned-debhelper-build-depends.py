#!/usr/bin/python3

from lintian_brush.control import (
    ensure_minimum_version,
    update_control,
    )


with open('debian/compat', 'r') as f:
    minimum_version = f.read().strip()


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
            control["Build-Depends"],
            "debhelper",
            minimum_version)


update_control(source_package_cb=bump_debhelper)
print("Bump debhelper dependency to >= %s, since that's what is "
      "used in debian/compat." % minimum_version)
print("Fixed-Lintian-Tags: package-needs-versioned-debhelper-build-depends")
