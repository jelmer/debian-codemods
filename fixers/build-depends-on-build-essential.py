#!/usr/bin/python3

from lintian_brush.control import (
    drop_dependency,
    update_control,
    )


def drop_build_essential(control):
    try:
        control["Build-Depends"] = drop_dependency(
            control["Build-Depends"], "build-essential")
    except KeyError:
        pass


update_control(source_package_cb=drop_build_essential)
print("Drop unnecessary dependency on build-essential.")
print("Fixed-Lintian-Tags: build-depends-on-build-essential")
