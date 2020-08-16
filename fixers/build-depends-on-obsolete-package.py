#!/usr/bin/python3
from debmutate.control import (
    drop_dependency,
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    )
from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    for field in ["Build-Depends", "Build-Depends-Indep",
                  "Build-Depends-Arch"]:
        try:
            old_build_depends = updater.source[field]
        except KeyError:
            continue
        updater.source[field] = drop_dependency(
            old_build_depends, "dh-systemd")
        if old_build_depends != updater.source[field]:
            ensure_minimum_debhelper_version(updater.source, "9.20160709")
        fixed_lintian_tag(
            updater.source, 'build-depends-on-obsolete-package',
            info='%s: %s' % (field.lower(), 'dh-systemd'))


report_result(
    "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd.")
