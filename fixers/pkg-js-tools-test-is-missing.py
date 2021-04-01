#!/usr/bin/python3

import os
import sys

from debmutate.control import ensure_some_version, ControlEditor
from debmutate.debhelper import get_sequences

from lintian_brush.fixer import report_result, LintianIssue, meets_minimum_certainty

CERTAINTY = 'possible'


if not meets_minimum_certainty(CERTAINTY):
    sys.exit(2)


with ControlEditor() as control_editor:
    sequences = list(get_sequences(control_editor=control_editor))

    if 'nodejs' not in sequences:
        sys.exit(2)

    issue = LintianIssue(
        control_editor.source,
        'pkg-js-tools-test-is-missing', 'debian/tests/pkg-js/test')
    if issue.should_fix():
        if os.path.exists("test/node.js"):
            os.makedirs("debian/tests", exist_ok=True)
            with open("debian/tests/pkg-js/test", "w") as f:
                f.write("mocha test/node.js\n")
            control_editor.source["Build-Depends"] = ensure_some_version(
                control_editor.source["Build-Depends"], "mocha <!nocheck>"
            )
            issue.report_fixed()
        elif os.path.exists("test.js"):
            os.makedirs("debian/tests/pkg-js", exist_ok=True)
            with open("debian/tests/pkg-js/test", "w") as f:
                f.write("tape test.js\n")
            control_editor.source["Build-Depends"] = ensure_some_version(
                control_editor.source["Build-Depends"], "node-tape <!nocheck>"
            )
            issue.report_fixed()


report_result('Add autopkgtest for node.', certainty=CERTAINTY)
