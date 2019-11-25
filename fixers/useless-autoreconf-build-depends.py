#!/usr/bin/python3
import sys

from lintian_brush.control import (
    drop_dependency,
    ensure_minimum_debhelper_version,
    update_control,
    )
from lintian_brush.rules import (
    dh_invoke_drop_with,
    update_rules,
    )


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_debhelper_version(
        control.get("Build-Depends", ""), "10")
    control["Build-Depends"] = drop_dependency(
        control["Build-Depends"],
        "dh-autoreconf")


def drop_with_autoreconf(line, target):
    return dh_invoke_drop_with(line, b'autoreconf')


changed = update_rules(drop_with_autoreconf)
if not changed:
    sys.exit(2)


update_control(source_package_cb=bump_debhelper)

print("Drop unnecessary dependency on dh-autoreconf.")
print("Fixed-Lintian-Tags: useless-autoreconf-build-depends")
