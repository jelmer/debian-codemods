#!/usr/bin/python3

from debmutate.control import drop_dependency, ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag

with open('debian/rules', 'rb') as f:
    for line in f:
        if b'/usr/share/cdbs/' in line:
            uses_cdbs = True
            break
    else:
        uses_cdbs = False

if not uses_cdbs:
    with ControlEditor() as updater:
        new_depends = drop_dependency(
            updater.source.get("Build-Depends", ""), "cdbs")
        if new_depends != updater.source['Build-Depends']:
            fixed_lintian_tag(
                updater.source, 'unused-build-dependency-on-cdbs', '')
            updater.source["Build-Depends"] = new_depends
        if not updater.source["Build-Depends"]:
            del updater.source["Build-Depends"]

report_result("Drop unused build-dependency on cdbs.")
