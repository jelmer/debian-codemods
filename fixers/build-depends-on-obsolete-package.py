#!/usr/bin/python3
from debmutate.control import drop_dependency
from debmutate.debhelper import ensure_minimum_debhelper_version
from lintian_brush.fixer import LintianIssue, control, report_result

with control as updater:
    for field in [
        "Build-Depends",
        "Build-Depends-Indep",
        "Build-Depends-Arch",
    ]:
        try:
            old_build_depends = updater.source[field]
        except KeyError:
            continue
        issue = LintianIssue(
            updater.source,
            "build-depends-on-obsolete-package",
            info="{}: {}".format(field.lower(), "dh-systemd"),
        )
        if not issue.should_fix():
            continue
        updater.source[field] = drop_dependency(
            old_build_depends, "dh-systemd"
        )
        if old_build_depends != updater.source[field]:
            ensure_minimum_debhelper_version(updater.source, "9.20160709")
            issue.report_fixed()


report_result(
    "Depend on newer debhelper (>= 9.20160709) rather than dh-systemd."
)
