#!/usr/bin/python3
from lintian_brush.control import (
    update_control,
    )
import urllib.parse
import urllib.request

known_https = [
    'github.com', 'launchpad.net', 'pypi.python.org',
    'pear.php.net', 'pecl.php.net', 'www.bioconductor.org',
    'cran.r-project.org', 'wiki.debian.org']


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
    # Fall back to just comparing the two
    http_contents = urllib.request.urlopen(http_url).read()
    https_contents = urllib.request.urlopen(https_url).read()
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
