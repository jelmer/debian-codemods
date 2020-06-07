#!/usr/bin/python3

from debmutate.control import ControlUpdater
from lintian_brush.vcs import fixup_broken_git_url


with ControlUpdater() as updater:
    if "Vcs-Git" in updater.source:
        updater.source['Vcs-Git'] = fixup_broken_git_url(
            updater.source["Vcs-Git"])


print("Fix broken Vcs URL.")
