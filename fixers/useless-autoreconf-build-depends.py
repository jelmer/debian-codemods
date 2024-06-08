#!/usr/bin/python3
import sys

from debmutate._rules import (
    dh_invoke_drop_with,
    update_rules,
)
from debmutate.control import (
    drop_dependency,
)
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
)
from lintian_brush.debhelper import maximum_debhelper_compat_version
from lintian_brush.fixer import (
    LintianIssue,
    compat_release,
    control,
    report_result,
)


def drop_with_autoreconf(line, target):
    return dh_invoke_drop_with(line, b"autoreconf")


if maximum_debhelper_compat_version(compat_release()) < 10:
    sys.exit(0)

if not update_rules(drop_with_autoreconf):
    sys.exit(2)


with control as updater:
    ensure_minimum_debhelper_version(updater.source, "10~")
    new_depends = drop_dependency(
        updater.source.get("Build-Depends", ""), "dh-autoreconf"
    )
    if new_depends != updater.source["Build-Depends"]:
        issue = LintianIssue(
            updater.source, "useless-autoreconf-build-depends", "dh-autoreconf"
        )
        if issue.should_fix():
            updater.source["Build-Depends"] = new_depends
            issue.report_fixed()

report_result("Drop unnecessary dependency on dh-autoreconf.")
