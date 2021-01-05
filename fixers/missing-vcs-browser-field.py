#!/usr/bin/python3

from lintian_brush.fixer import control, report_result, fixed_lintian_tag
from lintian_brush.vcs import determine_browser_url


with control as updater:
    if "Vcs-Browser" not in updater.source:
        try:
            vcs_git = updater.source["Vcs-Git"]
        except KeyError:
            pass
        else:
            browser_url = determine_browser_url('git', vcs_git)
            if browser_url is not None:
                updater.source["Vcs-Browser"] = browser_url
                fixed_lintian_tag(
                    updater.source, 'missing-vcs-browser-field',
                    info='Vcs-Git %s' % vcs_git)


report_result("debian/control: Add Vcs-Browser field")
