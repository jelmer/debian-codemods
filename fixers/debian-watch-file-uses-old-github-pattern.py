#!/usr/bin/python3

from debmutate.watch import WatchEditor
from urllib.parse import urlparse

from lintian_brush.fixer import report_result


with WatchEditor() as updater:
    for w in getattr(updater.watch_file, "entries", []):
        parsed_url = urlparse(w.url)

        # only applies to github.com
        if parsed_url.netloc != "github.com":
            continue

        parts = parsed_url.path.strip('/').split('/')
        if len(parts) < 3 or parts[2] not in ('tags', 'releases'):
            continue

        parts = w.matching_pattern.split('/')
        if parts[-2] == 'archive':
            parts.insert(-1, 'refs/tags')
        w.matching_pattern = '/'.join(parts)


report_result(
    "Update pattern for GitHub archive URLs from /<org>/<repo>/tags page"
    "/<org>/<repo>/archive/<tag> -> /<org>/<repo>/archive/refs/tags/<tag>."
)
