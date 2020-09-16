#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, fixed_lintian_tag


with ControlEditor() as updater:
    if updater.source.get("Testsuite") == "autopkgtest":
        del updater.source["Testsuite"]
        fixed_lintian_tag(
            updater.source, 'unnecessary-testsuite-autopkgtest-field')

report_result("Remove unnecessary 'Testsuite: autopkgtest' header.")
