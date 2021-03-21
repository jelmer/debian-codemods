#!/usr/bin/python3

from debmutate.watch import WatchEditor
from urllib.parse import urlparse

from lintian_brush.fixer import report_result, LintianIssue


with WatchEditor() as updater:
    for w in getattr(updater.watch_file, 'entries', []):
        parsed_url = urlparse(w.url)
        if parsed_url.hostname != 'github.com':
            continue
        parts = parsed_url.path.strip('/').split('/')
        if len(parts) < 3 or parts[2] != 'tags':
            continue
        if w.matching_pattern == ".*/archive/(.*)\\.tar\\.gz":
            w.matching_pattern = ".*/archive/.*/(.*)\\.tar\\.gz"


report_result(
    'Update pattern for GitHub URL in watch file to cope with '
    'addition of full ref name.')
