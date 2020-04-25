#!/usr/bin/python3

from lintian_brush.control import ControlUpdater
from lintian_brush.fixer import report_result


with ControlUpdater() as updater:
    if updater.source.get("Testsuite") == "autopkgtest":
        del updater.source["Testsuite"]

report_result(
    "Remove unnecessary 'Testsuite: autopkgtest' header.",
    fixed_lintian_tags=['unnecessary-testsuite-autopkgtest-field'])
