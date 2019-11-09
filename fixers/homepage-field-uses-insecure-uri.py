#!/usr/bin/python3
from lintian_brush import USER_AGENT, DEFAULT_URLLIB_TIMEOUT
from lintian_brush.control import (
    update_control,
    )
import os
import socket
import sys
import urllib.error
import urllib.parse
from urllib.request import urlopen, Request

known_https = [
    'github.com', 'launchpad.net', 'pypi.python.org',
    'pear.php.net', 'pecl.php.net', 'www.bioconductor.org',
    'cran.r-project.org', 'wiki.debian.org']

ERRORS = (urllib.error.URLError, urllib.error.HTTPError, ConnectionResetError,
          socket.timeout)


def same_page(http_contents, https_contents):
    # This is a pretty crude way to determine we end up on the same page, but
    # it works.
    http_contents = http_contents.replace(b'https', b'').replace(b'http', b'')
    https_contents = https_contents.replace(b'https', b'').replace(
            b'http', b'')
    return http_contents == https_contents


def fix_homepage(http_url):
    if not http_url.startswith('http:'):
        return http_url
    https_url = 'https:' + http_url[len('http:'):]
    result = urllib.parse.urlparse(http_url)
    if result.netloc in known_https:
        return https_url
    if os.environ.get('NET_ACCESS', 'allow') != 'allow':
        return http_url
    # Fall back to just comparing the two
    headers = {'User-Agent': USER_AGENT}
    try:
        http_contents = urlopen(
            Request(http_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT).read()
    except ERRORS as e:
        sys.stderr.write(
            'Unable to access HTTP version of homepage %s: %s' %
            (http_url, e))
        return http_url
    try:
        https_contents = urlopen(
            Request(https_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT).read()
    except ERRORS as e:
        sys.stderr.write(
            'Unable to access HTTPS version of homepage %s: %s' %
            (https_url, e))
        return http_url
    if same_page(http_contents, https_contents):
        return https_url
    return http_url


def fix_homepage_header(control):
    try:
        homepage = control["Homepage"]
    except KeyError:
        return
    control["Homepage"] = fix_homepage(homepage)


update_control(source_package_cb=fix_homepage_header)

print("Use secure URI in Homepage field.")
print("Fixed-Lintian-Tags: homepage-field-uses-insecure-uri")
