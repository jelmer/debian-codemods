#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import (
    report_result, source_package_name, current_package_version,
)
from lintian_brush.watch import (
    fix_old_github_patterns,
    watch_entries_certainty,
)

certainty = "certain"


try:
    with WatchEditor() as updater:
        changed_entries = fix_old_github_patterns(updater)
        certainty = watch_entries_certainty(
            changed_entries, source_package_name(),
            expected_versions=[current_package_version().upstream_version])
except FileNotFoundError:
    pass


report_result(
    "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page"
    "/<org>/<repo>/archive/<tag> → /<org>/<repo>/archive/refs/tags/<tag>.",
    certainty=certainty
)
