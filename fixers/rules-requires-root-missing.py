#!/usr/bin/python3

from debmutate.control import ( ControlEditor )
from lintian_brush.fixer import report_result

require_root = "no"
updated = False

with ControlEditor() as updater:
    if "Rules-Requires-Root" not in updater.source:
        # XXX add some heuristics to set require_root = "yes" in common
        # cases, like `debian/rules binary` chown(1)'ing stuff
        updater.source["Rules-Requires-Root"] = require_root
        updated = True

report_result(
    "Set Rules-Requires-Root: %s." % require_root,
    fixed_lintian_tags=["rules-requires-root-missing"])

if require_root == "no" and updated == True:
    with open("/dev/tty", "w") as p:
      # ugly work around lintian-brush capturing the error output 
      p.write("NOTE: Don't forget to verify that the binaries built with this field present are identical!\n")
