#!/usr/bin/python3

from lintian_brush.control import (
    drop_dependency,
    update_control,
    )


def drop_build_essential(control):
    control["Build-Depends"] = drop_dependency(
        control.get("Build-Depends", ""), "build-essential")


update_control(source_package_cb=drop_build_essential)
print("Drop unnecessary dependency on build-essential.")
print("Fixed-Lintian-Tags: build-depends-on-build-essential")
