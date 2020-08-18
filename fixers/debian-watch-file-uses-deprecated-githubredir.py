#!/usr/bin/python3

from debmutate.watch import WatchEditor
from urllib.parse import urlparse

from lintian_brush.fixer import report_result, fixed_lintian_tag


with WatchEditor() as updater:
    for w in getattr(updater.watch_file, 'entries', []):
        parsed_url = urlparse(w.url)
        if parsed_url.netloc != 'githubredir.debian.net':
            continue
        parts = parsed_url.path.strip('/').split('/')
        if parts[0] != 'github':
            # Hmm.
            continue
        fixed_lintian_tag(
            'source', 'debian-watch-file-uses-deprecated-githubredir',
            info='%s %s' % (w.url, w.matching_pattern))
        w.url = 'https://github.com/%s/%s/tags' % (parts[1], parts[2])
        w.matching_pattern = '.*/' + w.matching_pattern.rsplit('/', 1)[-1]


report_result(
    'Remove use of githubredir - see '
    'https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html '
    'for details.')
