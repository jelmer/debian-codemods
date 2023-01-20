#!/usr/bin/python3

from debmutate.watch import WatchEditor
from urllib.parse import urlparse

from lintian_brush.fixer import (
    report_result, LintianIssue,
    net_access_allowed,
    source_package_name,
    current_package_version,
)
from lintian_brush.watch import watch_entries_certainty


try:
    with WatchEditor() as updater:
        changed_entries = []
        for w in getattr(updater.watch_file, 'entries', []):
            parsed_url = urlparse(w.url)
            if parsed_url.netloc != 'githubredir.debian.net':
                continue
            parts = parsed_url.path.strip('/').split('/')
            if parts[0] != 'github':
                # Hmm.
                continue
            issue = LintianIssue(
                'source', 'debian-watch-file-uses-deprecated-githubredir',
                info=f'{w.url} {w.matching_pattern}')
            if issue.should_fix():
                w.url = f'https://github.com/{parts[1]}/{parts[2]}/tags'
                w.matching_pattern = (
                    '.*/' + w.matching_pattern.rsplit('/', 1)[-1])
                issue.report_fixed()
                changed_entries.append(w)
        if net_access_allowed():
            certainty = watch_entries_certainty(
                changed_entries, source_package_name(),
                expected_versions=[current_package_version().upstream_version],
                default_certainty="confident")
        else:
            certainty = "confident"
except FileNotFoundError:
    pass
else:
    report_result(
        'Remove use of githubredir - see '
        'https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html '
        'for details.', certainty=certainty)
