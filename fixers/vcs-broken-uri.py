#!/usr/bin/python3

from debmutate.control import ControlEditor
from lintian_brush.fixer import report_result
from lintian_brush.vcs import fixup_broken_git_url


with ControlEditor() as updater:
    if "Vcs-Git" in updater.source:
        updater.source['Vcs-Git'] = fixup_broken_git_url(
            updater.source["Vcs-Git"])


report_result("Fix broken Vcs URL.")
