#!/usr/bin/python3

from lintian_brush.control import update_control
from lintian_brush.vcs import fixup_broken_git_url


def fix_broken_vcs_url(control):
    if "Vcs-Git" in control:
        parts = control["Vcs-Git"].split(' ')
        url = fixup_broken_git_url(parts[0])
        control["Vcs-Git"] = ' '.join([url] + parts[1:])


update_control(source_package_cb=fix_broken_vcs_url)

print("Fix broken Vcs URL.")
