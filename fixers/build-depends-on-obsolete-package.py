#!/usr/bin/python3
from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    update_control,
    )


def bump_debhelper(control):
    try:
        old_build_depends = control["Build-Depends"]
    except KeyError:
        return
    control["Build-Depends"] = drop_dependency(
        old_build_depends, "dh-systemd")
    if old_build_depends != control["Build-Depends"]:
        control["Build-Depends"] = ensure_minimum_debhelper_version(
            control["Build-Depends"], "9.20160709")


if update_control(source_package_cb=bump_debhelper):
    print("Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.")
print("Fixed-Lintian-Tags: build-depends-on-obsolete-package")
