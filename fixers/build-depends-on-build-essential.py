#!/usr/bin/python3

from lintian_brush.control import (
    drop_dependency,
    update_control,
    )
from lintian_brush.fixer import report_result


def drop_build_essential(control):
    try:
        control["Build-Depends"] = drop_dependency(
            control["Build-Depends"], "build-essential")
    except KeyError:
        pass


update_control(source_package_cb=drop_build_essential)

report_result(
    "Drop unnecessary dependency on build-essential.",
    fixed_lintian_tags=['build-depends-on-build-essential'])
