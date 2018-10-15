#!/usr/bin/python3

from lintian_brush.control import update_control

def fix_priority(control):
    if control.get("Priority") == "optional":
        control["Priority"] = "extra"

update_control(binary_package_cb=fix_priority, source_package_cb=fix_priority)
print("Change priority optional to priority extra.")
