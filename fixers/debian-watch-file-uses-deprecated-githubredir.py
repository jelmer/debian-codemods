#!/usr/bin/python3

from lintian_brush.watch import update_watch
from urllib.parse import urlparse


def remove_githubredir(w):
    parsed_url = urlparse(w.url)
    if parsed_url.netloc != 'githubredir.debian.net':
        return
    parts = parsed_url.path.strip('/').split('/')
    if parts[0] != 'github':
        # Hmm.
        return
    w.url = 'https://github.com/%s/%s/tags' % (parts[1], parts[2])
    w.matching_pattern = '.*/' + w.matching_pattern.rsplit('/', 1)[-1]


update_watch(remove_githubredir)

print(
    'Remove use of githubredir - see '
    'https://lists.debian.org/debian-devel-announce/2014/10/msg00000.html '
    'for details.')
print('Fixed-Lintian-Tags: debian-watch-file-uses-deprecated-githubredir')
