#!/usr/bin/python3
import sys

from debmutate.control import (
    drop_dependency,
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
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


with ControlEditor() as updater:
    ensure_minimum_debhelper_version(updater.source, "10~")
    updater.source["Build-Depends"] = drop_dependency(
        updater.source["Build-Depends"], "dh-autoreconf")

report_result(
    "Drop unnecessary dependency on dh-autoreconf.",
    fixed_lintian_tags=['useless-autoreconf-build-depends'])
