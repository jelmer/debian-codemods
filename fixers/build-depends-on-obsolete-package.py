#!/usr/bin/python3
from debian.changelog import Version

from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_version,
    update_control,
    )


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
        control["Build-Depends"],
        "debhelper", Version("9.20160709"))
    control["Build-Depends"] = drop_dependency(
        control["Build-Depends"],
        "dh-systemd")


if update_control(source_package_cb=bump_debhelper):
    print("Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.")
print("Fixed-Lintian-Tags: build-depends-on-obsolete-package")
