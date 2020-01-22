#!/usr/bin/python3
from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    update_control,
    )
from lintian_brush.fixer import report_result


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
    report_result(
        "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.",
        fixed_lintian_tags=['build-depends-on-obsolete-package'])
