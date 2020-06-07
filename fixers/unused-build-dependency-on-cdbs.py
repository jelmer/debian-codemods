#!/usr/bin/python3

from debmutate.control import drop_dependency, ControlEditor

with open('debian/rules', 'rb') as f:
    for line in f:
        if b'/usr/share/cdbs/' in line:
            uses_cdbs = True
            break
    else:
        uses_cdbs = False

if not uses_cdbs:
    with ControlEditor() as updater:
        updater.source["Build-Depends"] = drop_dependency(
            updater.source.get("Build-Depends", ""), "cdbs")
        if not updater.source["Build-Depends"]:
            del updater.source["Build-Depends"]

print("Drop unused build-dependency on cdbs.")
print("Fixed-Lintian-Tags: unused-build-dependency-on-cdbs")
