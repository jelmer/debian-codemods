#!/usr/bin/python3
from lintian_brush import USER_AGENT, DEFAULT_URLLIB_TIMEOUT
from lintian_brush.fixer import (
    control, net_access_allowed, report_result, LintianIssue,
    warn)
import socket
import http.client
import sys
import urllib.error
import urllib.parse
from urllib.request import urlopen, Request

known_https = [
    'github.com', 'launchpad.net', 'pypi.python.org',
    'pear.php.net', 'pecl.php.net', 'www.bioconductor.org',
    'cran.r-project.org', 'wiki.debian.org']

ERRORS = (urllib.error.URLError, urllib.error.HTTPError, ConnectionResetError,
          socket.timeout, http.client.BadStatusLine)


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
    if not net_access_allowed():
        return http_url
    # Fall back to just comparing the two
    headers = {'User-Agent': USER_AGENT}
    try:
        http_contents = urlopen(
            Request(http_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT).read()
    except ERRORS as e:
        warn(
            'Unable to access HTTP version of homepage %s: %s' %
            (http_url, e))
        return http_url
    try:
        https_resp = urlopen(
            Request(https_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except ERRORS as e:
        warn(
            'Unable to access HTTPS version of homepage %s: %s' %
            (https_url, e))
        return http_url
    if not https_resp.geturl().startswith('https://'):
        warn('https URL {} redirected back to {}'.format(
            https_url, https_resp.geturl()))
        return http_url

    https_contents = https_resp.read()
    if same_page(http_contents, https_contents):
        return https_url
    return http_url


try:
    with control as updater:
        try:
            homepage = updater.source["Homepage"]
        except KeyError:
            pass
        else:
            new_homepage = fix_homepage(homepage)
            if new_homepage != updater.source['Homepage']:
                issue = LintianIssue(
                    'source', 'homepage-field-uses-insecure-uri',
                    updater.source['Homepage'])
                if issue.should_fix():
                    updater.source["Homepage"] = new_homepage
                    issue.report_fixed()
except FileNotFoundError:
    sys.exit(0)


report_result("Use secure URI in Homepage field.")
