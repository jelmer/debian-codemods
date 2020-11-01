#!/usr/bin/python3

import sys

from debian.changelog import Version
from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result, LintianIssue, compat_release
from lintian_brush.release_info import key_package_version


# Don't upgrade any package that needs to be compatible with dpkg < 1.17.1
dpkg_version = key_package_version('dpkg', compat_release())
if dpkg_version is None or dpkg_version < Version('1.17.1'):
    sys.exit(0)


with ControlEditor() as updater:
    if updater.source.get("Testsuite") == "autopkgtest":
        issue = LintianIssue(
            updater.source, 'unnecessary-testsuite-autopkgtest-field')
        if issue.should_fix():
            del updater.source["Testsuite"]
            issue.report_fixed()


report_result("Remove unnecessary 'Testsuite: autopkgtest' header.")
