#!/usr/bin/python3

from debian.changelog import Version
from lintian_brush.control import (
    ensure_minimum_version,
    update_control,
    )
from lintian_brush.rules import (
    update_rules,
    dh_invoke_drop_with,
    )


def cb(line):
    if line == b'dh_autotools-dev_updateconfig':
        return []
    if line == b'dh_autotools-dev_restoreconfig':
        return []
    return dh_invoke_drop_with(line, b'autotools-dev')


def bump_debhelper(control):
    control["Build-Depends"] = ensure_minimum_version(
        control["Build-Depends"],
        "debhelper", Version("9.20160114"))


update_rules(cb)
update_control(source_package_cb=bump_debhelper)
print("Drop use of autotools-dev debhelper.")
print("Fixed-Lintian-Tags: debhelper-tools-from-autotools-dev-are-deprecated")
