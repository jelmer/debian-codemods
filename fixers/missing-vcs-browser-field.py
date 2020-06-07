#!/usr/bin/python3

from debmutate.control import ControlEditor

from lintian_brush.fixer import report_result
from lintian_brush.vcs import determine_browser_url


with ControlEditor() as updater:
    if "Vcs-Browser" not in updater.source:
        try:
            vcs_git = updater.source["Vcs-Git"]
        except KeyError:
            pass
        else:
            browser_url = determine_browser_url('git', vcs_git)
            if browser_url is not None:
                updater.source["Vcs-Browser"] = browser_url


report_result(
    "debian/control: Add Vcs-Browser field",
    fixed_lintian_tags=['missing-vcs-browser-field'])
