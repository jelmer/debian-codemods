#!/usr/bin/python3
from debmutate.control import (
    drop_dependency,
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    )
from lintian_brush.fixer import report_result
import sys


with ControlEditor() as updater:
    try:
        old_build_depends = updater.source["Build-Depends"]
    except KeyError:
        sys.exit(0)
    updater.source["Build-Depends"] = drop_dependency(
        old_build_depends, "dh-systemd")
    if old_build_depends != updater.source["Build-Depends"]:
        ensure_minimum_debhelper_version(updater.source, "9.20160709")


report_result(
    "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.",
    fixed_lintian_tags=['build-depends-on-obsolete-package'])
