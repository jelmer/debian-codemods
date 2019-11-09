#!/usr/bin/python3

from lintian_brush.control import update_control
import re
from urllib.parse import urlparse, urlunparse


def canonicalize_vcs_browser_url(url):
    url = url.replace(
        "https://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/")
    url = url.replace(
        "http://svn.debian.org/wsvn/",
        "https://anonscm.debian.org/viewvc/")
    url = url.replace(
        "https://git.debian.org/?p=",
        "https://anonscm.debian.org/git/")
    url = url.replace(
        "http://git.debian.org/?p=",
        "https://anonscm.debian.org/git/")
    url = url.replace(
        "https://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/")
    url = url.replace(
        "http://bzr.debian.org/loggerhead/",
        "https://anonscm.debian.org/loggerhead/")
    url = re.sub(
        r"^https?://salsa.debian.org/([^/]+/[^/]+)\.git/?$",
        "https://salsa.debian.org/\\1",
        url)
    return url


def canonical_vcs_git_url(url):
    parsed_url = urlparse(url)
    if parsed_url.netloc == 'salsa.debian.org':
        if not parsed_url.path.rstrip('/').endswith('.git'):
            parsed_url = parsed_url._replace(
                path=parsed_url.path.rstrip('/') + '.git')
        return urlunparse(parsed_url)
    return url


canonicalize_vcs = {
    'Browser': canonicalize_vcs_browser_url,
    'Git': canonical_vcs_git_url,
}

fields = set()


def canonicalize_urls(control):
    for kind, fn in canonicalize_vcs.items():
        if ("Vcs-" + kind) in control:
            control["Vcs-" + kind] = fn(control["Vcs-" + kind])
            fields.add("Vcs-" + kind)


update_control(source_package_cb=canonicalize_urls)

print("Use canonical URL in " + ', '.join(sorted(fields)) + '.')
print("Fixed-Lintian-Tags: vcs-field-not-canonical")
