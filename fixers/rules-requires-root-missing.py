#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import (
    report_result,
    meets_minimum_certainty,
    fixed_lintian_tag,
    )
import sys

require_root = "no"
CERTAINTY = "possible"

if not meets_minimum_certainty(CERTAINTY):
    sys.exit(0)

with ControlEditor() as updater:
    if "Rules-Requires-Root" not in updater.source:
        # TODO: add some heuristics to set require_root = "yes" in common
        # cases, like `debian/rules binary` chown(1)'ing stuff
        updater.source["Rules-Requires-Root"] = require_root
        fixed_lintian_tag(updater.source, 'silent-on-rules-requiring-root')

report_result(
    "Set Rules-Requires-Root: %s." % require_root,
    certainty=CERTAINTY)
