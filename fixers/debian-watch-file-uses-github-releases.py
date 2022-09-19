#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import report_result
from lintian_brush.watch import fix_github_releases


with WatchEditor() as updater:
    fix_github_releases(updater)


report_result(
    "debian/watch: Use GitHub /tags rather than /releases page.")
