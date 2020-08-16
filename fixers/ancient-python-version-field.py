#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    fixed_lintian_tag,
    report_result,
    )


with ControlEditor() as updater:
    # Remove anything that involves python 2.6, 2.7, 3.3
    if "X-Python-Version" in updater.source:
        info = 'x-python-version %s' % updater.source['X-Python-Version']
        if updater.source["X-Python-Version"].strip().startswith(">= 2."):
            del updater.source["X-Python-Version"]
            fixed_lintian_tag(updater.source, 'ancient-python-version-field',
                              info=info)
    if ("X-Python3-Version" in updater.source and
            updater.source["X-Python3-Version"].strip().startswith(">=")):
        vers = updater.source["X-Python3-Version"].split(">=")[1].strip()
        info = 'x-python3-version %s' % updater.source['X-Python3-Version']
        if vers in ("3.0", "3.1", "3.2", "3.3", "3.4"):
            del updater.source["X-Python3-Version"]
            fixed_lintian_tag(
                updater.source, 'ancient-python-version-field', info=info)


report_result(
    "Remove unnecessary X-Python{,3}-Version field in debian/control.")
