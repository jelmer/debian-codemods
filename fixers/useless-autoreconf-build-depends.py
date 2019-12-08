#!/usr/bin/python3
import os
import sys

from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    update_control,
    )
from lintian_brush.debhelper import maximum_debhelper_compat_version
from lintian_brush.rules import (
    dh_invoke_drop_with,
    update_rules,
    )

compat_release = os.environ.get('COMPAT_RELEASE', 'sid')


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_debhelper_version(
        control.get("Build-Depends", ""), "10")
    control["Build-Depends"] = drop_dependency(
        control["Build-Depends"],
        "dh-autoreconf")


def drop_with_autoreconf(line, target):
    return dh_invoke_drop_with(line, b'autoreconf')


if maximum_debhelper_compat_version(compat_release) < 10:
    sys.exit(0)

changed = update_rules(drop_with_autoreconf)
if not changed:
    sys.exit(2)


update_control(source_package_cb=bump_debhelper)

print("Drop unnecessary dependency on dh-autoreconf.")
print("Fixed-Lintian-Tags: useless-autoreconf-build-depends")
