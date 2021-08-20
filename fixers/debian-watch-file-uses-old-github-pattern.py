#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import report_result
from lintian_brush.watch import fix_old_github_patterns


with WatchEditor() as updater:
    fix_old_github_patterns(updater)


report_result(
    "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page"
    "/<org>/<repo>/archive/<tag> -> /<org>/<repo>/archive/refs/tags/<tag>."
)
