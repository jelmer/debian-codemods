#!/usr/bin/python3

from lintian_brush.control import update_control


def fix_priority(control):
    if control.get("Priority") == "extra":
        control["Priority"] = "optional"


update_control(binary_package_cb=fix_priority, source_package_cb=fix_priority)
print("Change priority extra to priority optional.")
print("Fixed-Lintian-Tags: priority-extra-is-replaced-by-priority-optional")
