#!/usr/bin/python3
import sys

from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    ControlUpdater,
    )
from lintian_brush.debhelper import maximum_debhelper_compat_version
from lintian_brush.fixer import compat_release, report_result
from lintian_brush.rules import (
    dh_invoke_drop_with,
    update_rules,
    )


def drop_with_autoreconf(line, target):
    return dh_invoke_drop_with(line, b'autoreconf')


if maximum_debhelper_compat_version(compat_release()) < 10:
    sys.exit(0)

if not update_rules(drop_with_autoreconf):
    sys.exit(2)


with ControlUpdater() as updater:
    updater.source["Build-Depends"] = ensure_minimum_debhelper_version(
        updater.source.get("Build-Depends", ""), "10~")
    updater.source["Build-Depends"] = drop_dependency(
        updater.source["Build-Depends"], "dh-autoreconf")

report_result(
    "Drop unnecessary dependency on dh-autoreconf.",
    fixed_lintian_tags=['useless-autoreconf-build-depends'])
