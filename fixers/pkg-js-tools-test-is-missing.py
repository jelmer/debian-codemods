#!/usr/bin/python3

import os
import sys

from debmutate.control import ensure_some_version
from debmutate.debhelper import get_sequences

from lintian_brush.fixer import (
    LintianIssue,
    control,
    meets_minimum_certainty,
    report_result,
)

CERTAINTY = 'possible'


if not meets_minimum_certainty(CERTAINTY):
    sys.exit(2)


with control:
    sequences = list(get_sequences(control_editor=control))

    if 'nodejs' not in sequences:
        sys.exit(2)

    issue = LintianIssue(
        control.source,
        'pkg-js-tools-test-is-missing', 'debian/tests/pkg-js/test')
    if issue.should_fix():
        if os.path.exists("test/node.js"):
            os.makedirs("debian/tests", exist_ok=True)
            with open("debian/tests/pkg-js/test", "w") as f:
                f.write("mocha test/node.js\n")
            control.source["Build-Depends"] = ensure_some_version(
                control.source.get("Build-Depends", ''), "mocha <!nocheck>"
            )
            issue.report_fixed()
        elif os.path.exists("test.js"):
            os.makedirs("debian/tests/pkg-js", exist_ok=True)
            with open("debian/tests/pkg-js/test", "w") as f:
                f.write("tape test.js\n")
            control.source["Build-Depends"] = ensure_some_version(
                control.source.get("Build-Depends", ''), "node-tape <!nocheck>"
            )
            issue.report_fixed()


report_result('Add autopkgtest for node.', certainty=CERTAINTY)
