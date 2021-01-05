#!/usr/bin/python3

import sys
from debmutate.control import drop_dependency
from lintian_brush.fixer import control, report_result, fixed_lintian_tag

try:
    with open('debian/rules', 'rb') as f:
        for line in f:
            if b'/usr/share/cdbs/' in line:
                uses_cdbs = True
                break
        else:
            uses_cdbs = False
except FileNotFoundError:
    # Unsure whether it actually needs cdbs
    sys.exit(2)

if not uses_cdbs:
    with control as updater:
        new_depends = drop_dependency(
            updater.source.get("Build-Depends", ""), "cdbs")
        if new_depends != updater.source['Build-Depends']:
            fixed_lintian_tag(
                updater.source, 'unused-build-dependency-on-cdbs', '')
            updater.source["Build-Depends"] = new_depends
        if not updater.source["Build-Depends"]:
            del updater.source["Build-Depends"]

report_result("Drop unused build-dependency on cdbs.")
