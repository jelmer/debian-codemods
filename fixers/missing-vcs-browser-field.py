#!/usr/bin/python3
from lintian_brush.control import ControlUpdater
from lintian_brush.vcs import determine_browser_url


with ControlUpdater() as updater:
    if "Vcs-Browser" not in updater.source:
        try:
            vcs_git = updater.source["Vcs-Git"]
        except KeyError:
            pass
        else:
            browser_url = determine_browser_url('git', vcs_git)
            if browser_url is not None:
                updater.source["Vcs-Browser"] = browser_url


print("debian/control: Add Vcs-Browser field")
print("Fixed-Lintian-Tags: missing-vcs-browser-field")
