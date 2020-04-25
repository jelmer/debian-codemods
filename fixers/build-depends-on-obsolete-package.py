#!/usr/bin/python3
from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    ControlUpdater,
    )
from lintian_brush.fixer import report_result
import sys


with ControlUpdater() as updater:
    try:
        old_build_depends = updater.source["Build-Depends"]
    except KeyError:
        sys.exit(0)
    updater.source["Build-Depends"] = drop_dependency(
        old_build_depends, "dh-systemd")
    if old_build_depends != updater.source["Build-Depends"]:
        updater.source["Build-Depends"] = ensure_minimum_debhelper_version(
            updater.source["Build-Depends"], "9.20160709")


report_result(
    "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.",
    fixed_lintian_tags=['build-depends-on-obsolete-package'])
