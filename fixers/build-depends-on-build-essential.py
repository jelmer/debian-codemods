#!/usr/bin/python3

from debmutate.control import (
    drop_dependency,
    ControlUpdater,
    )
from lintian_brush.fixer import report_result


with ControlUpdater() as updater:
    try:
        updater.source["Build-Depends"] = drop_dependency(
            updater.source["Build-Depends"], "build-essential")
    except KeyError:
        pass


report_result(
    "Drop unnecessary dependency on build-essential.",
    fixed_lintian_tags=['build-depends-on-build-essential'])
