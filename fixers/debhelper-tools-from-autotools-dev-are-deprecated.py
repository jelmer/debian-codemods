#!/usr/bin/python3

from lintian_brush.control import (
    ensure_minimum_debhelper_version,
    update_control,
    )
from lintian_brush.rules import (
    update_rules,
    dh_invoke_drop_with,
    )


def cb(line, target):
    if line == b'dh_autotools-dev_updateconfig':
        return []
    if line == b'dh_autotools-dev_restoreconfig':
        return []
    return dh_invoke_drop_with(line, b'autotools-dev')


def bump_debhelper(source):
    source["Build-Depends"] = ensure_minimum_debhelper_version(
        source.get("Build-Depends", ""), "9.20160114")


if update_rules(cb):
    update_control(source_package_cb=bump_debhelper)
print("Drop use of autotools-dev debhelper.")
print("Fixed-Lintian-Tags: debhelper-tools-from-autotools-dev-are-deprecated")
