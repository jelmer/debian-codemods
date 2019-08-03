#!/usr/bin/python3

import os
from lintian_brush import USER_AGENT
from lintian_brush.control import update_control
from lintian_brush.salsa import (
    determine_browser_url,
    guess_repository_url,
    salsa_url_from_alioth_url,
    )
from email.utils import parseaddr
from urllib.parse import urlparse
from urllib.request import urlopen, Request


OBSOLETE_HOSTS = [
    'anonscm.debian.org', 'alioth.debian.org', 'svn.debian.org',
    'git.debian.org', 'bzr.debian.org', 'hg.debian.org']


def is_on_obsolete_host(url):
    parsed_url = urlparse(url)
    host = parsed_url.netloc.split('@')[-1]
    return host in OBSOLETE_HOSTS


def get_vcs_info(control):
    if "Vcs-Git" in control:
        return ("git", control["Vcs-Git"].split(' ')[0])

    if "Vcs-Bzr" in control:
        return ("bzr", control["Vcs-Bzr"])

    if "Vcs-Svn" in control:
        return ("svn", control["Vcs-Svn"])

    if "Vcs-Hg" in control:
        return ("hg", control["Vcs-Hg"])

    return None, None


def verify_salsa_repository(url):
    headers = {'User-Agent': USER_AGENT}
    browser_url = determine_browser_url(url)
    response = urlopen(Request(browser_url, headers=headers))
    return response.status == 200


def migrate_from_obsolete_infra(control):
    vcs_type, vcs_url = get_vcs_info(control)
    if not is_on_obsolete_host(vcs_url):
        return

    package = control["Source"]
    maintainer_email = parseaddr(control["Maintainer"])[1]
    salsa_url = guess_repository_url(package, maintainer_email)
    if salsa_url is None:
        salsa_url = salsa_url_from_alioth_url(vcs_type, vcs_url)

    # Verify that there is actually a repository there
    if os.environ.get('SALSA_PROBE', 'enabled') == 'ensabled':
        if not verify_salsa_repository(salsa_url):
            return

    for hdr in ["Vcs-Git", "Vcs-Bzr", "Vcs-Hg", "Vcs-Svn", "Vcs-Browser"]:
        try:
            del control[hdr]
        except KeyError:
            pass
    control["Vcs-Git"] = salsa_url
    control["Vcs-Browser"] = determine_browser_url(salsa_url)


update_control(source_package_cb=migrate_from_obsolete_infra)
print("Update Vcs-* headers to use salsa repository.")
print("Fixed-Lintian-Tags: vcs-obsolete-in-debian-infrastructure")
