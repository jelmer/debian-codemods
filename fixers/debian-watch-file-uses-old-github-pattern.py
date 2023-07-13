#!/usr/bin/python3

from contextlib import suppress

from debmutate.watch import WatchEditor

from lintian_brush.fixer import (
    current_package_version,
    net_access_allowed,
    report_result,
    source_package_name,
)
from lintian_brush.watch import (
    fix_old_github_patterns,
    watch_entries_certainty,
)

certainty = "certain"


with suppress(FileNotFoundError), WatchEditor() as updater:
    changed_entries = fix_old_github_patterns(updater)
    if net_access_allowed():
        certainty = watch_entries_certainty(
            changed_entries, source_package_name(),
            expected_versions=[current_package_version().upstream_version])
    else:
        certainty = "likely"


report_result(
    "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page"
    "/<org>/<repo>/archive/<tag> → /<org>/<repo>/archive/refs/tags/<tag>.",
    certainty=certainty
)
