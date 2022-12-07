#!/usr/bin/python3

from debmutate.watch import WatchEditor

from lintian_brush.fixer import (
    report_result, source_package_name, current_package_version,
    net_access_allowed,
)
from lintian_brush.watch import fix_github_releases, watch_entries_certainty


try:
    with WatchEditor() as updater:
        entries = fix_github_releases(updater)
        if net_access_allowed():
            certainty = watch_entries_certainty(
                entries, source_package_name(),
                expected_versions=[current_package_version().upstream_version])
        else:
            certainty = None
except FileNotFoundError:
    pass
else:
    report_result(
        "debian/watch: Use GitHub /tags rather than /releases page.",
        certainty=certainty)
