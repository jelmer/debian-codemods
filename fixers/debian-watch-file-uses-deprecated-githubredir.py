#!/usr/bin/python3

from lintian_brush.watch import WatchUpdater
from urllib.parse import urlparse


with WatchUpdater() as updater:
    for w in getattr(updater.watch_file, 'entries', []):
        parsed_url = urlparse(w.url)
        if parsed_url.netloc != 'githubredir.debian.net':
            continue
        parts = parsed_url.path.strip('/').split('/')
        if parts[0] != 'github':
            # Hmm.
            continue
        w.url = 'https://github.com/%s/%s/tags' % (parts[1], parts[2])
        w.matching_pattern = '.*/' + w.matching_pattern.rsplit('/', 1)[-1]


print(
    'Remove use of githubredir - see '
    'https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html '
    'for details.')
print('Fixed-Lintian-Tags: debian-watch-file-uses-deprecated-githubredir')
