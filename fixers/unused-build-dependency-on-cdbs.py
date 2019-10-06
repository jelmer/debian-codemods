#!/usr/bin/python3

from lintian_brush.control import drop_dependency, update_control

with open('debian/rules', 'r') as f:
    for line in f:
        if '/usr/share/cdbs/' in line:
            uses_cdbs = True
            break
    else:
        uses_cdbs = False

if not uses_cdbs:
    def drop_cdbs(control):
        control["Build-Depends"] = drop_dependency(
            control.get("Build-Depends", ""), "cdbs")
        if not control["Build-Depends"]:
            del control["Build-Depends"]
    update_control(source_package_cb=drop_cdbs)

print("Drop unused build-dependency on cdbs.")
print("Fixed-Lintian-Tags: unused-build-dependency-on-cdbs")
