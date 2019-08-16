#!/usr/bin/python3
from lintian_brush.control import update_control
from lintian_brush.vcs import determine_browser_url


def add_vcs_browser(control):
    if "Vcs-Browser" in control:
        return
    try:
        vcs_git = control["Vcs-Git"]
    except KeyError:
        return
    browser_url = determine_browser_url('git', vcs_git)
    if browser_url is not None:
        control["Vcs-Browser"] = browser_url


update_control(source_package_cb=add_vcs_browser)
print("debian/control: Add Vcs-Browser field")
print("Fixed-Lintian-Tags: missing-vcs-browser-field")
