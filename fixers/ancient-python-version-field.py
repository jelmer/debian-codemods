#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result


with ControlEditor() as updater:
    # Remove anything that involves python 2.6, 2.7, 3.3
    if "X-Python-Version" in updater.source:
        if updater.source["X-Python-Version"].strip().startswith(">= 2."):
            del updater.source["X-Python-Version"]
    if ("X-Python3-Version" in updater.source and
            updater.source["X-Python3-Version"].strip().startswith(">=")):
        vers = updater.source["X-Python3-Version"].split(">=")[1].strip()
        if vers in ("3.0", "3.1", "3.2", "3.3", "3.4"):
            del updater.source["X-Python3-Version"]


report_result(
    "Remove unnecessary X-Python{,3}-Version field in debian/control.",
    fixed_lintian_tags=['ancient-python-version-field'])
