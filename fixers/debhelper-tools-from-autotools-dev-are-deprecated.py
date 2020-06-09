#!/usr/bin/python3

from debmutate.control import (
    ControlEditor,
    )
from debmutate.debhelper import (
    ensure_minimum_debhelper_version,
    )
from lintian_brush.fixer import report_result
from lintian_brush.rules import (
    update_rules,
    dh_invoke_drop_with,
    )


def cb(line, target):
    if line.strip() == b'dh_autotools-dev_updateconfig':
        return []
    if line.strip() == b'dh_autotools-dev_restoreconfig':
        return []
    line = dh_invoke_drop_with(line, b'autotools-dev')
    line = dh_invoke_drop_with(line, b'autotools_dev')
    return line


if update_rules(cb):
    with ControlEditor() as updater:
        ensure_minimum_debhelper_version(updater.source, "9.20160114")


report_result(
    "Drop use of autotools-dev debhelper.",
    fixed_lintian_tags=['debhelper-tools-from-autotools-dev-are-deprecated'])
