#!/usr/bin/python3

from lintian_brush.fixer import control, report_result
from lintian_brush.vcs import fixup_broken_git_url


with control as updater:
    if "Vcs-Git" in updater.source:
        updater.source['Vcs-Git'] = fixup_broken_git_url(
            updater.source["Vcs-Git"])


report_result("Fix broken Vcs URL.")
