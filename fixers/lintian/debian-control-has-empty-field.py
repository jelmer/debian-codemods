#!/usr/bin/python
from lintian_brush.control import update_control
import sys
removed = []
def rm_empty_field(control):
    for k, v in control.items():
        if not v.strip():
            removed.append(k)
            del control[k]
update_control(source_package_cb=rm_empty_field, binary_package_cb=rm_empty_field)
print "debian/control: Remove empty control field%s %s." % (
    "s" if len(removed) > 1 else "",
    ", ".join(removed),
    )
