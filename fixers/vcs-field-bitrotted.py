#!/usr/bin/python3

import asyncio
import os
import re
import sys
from lintian_brush import USER_AGENT, DEFAULT_URLLIB_TIMEOUT
from lintian_brush.control import update_control
from lintian_brush.salsa import (
    determine_browser_url as determine_salsa_browser_url,
    guess_repository_url,
    salsa_url_from_alioth_url,
    )
from lintian_brush.vcs import (
    determine_browser_url,
    split_vcs_url,
    )
from lintian_brush.vcswatch import VcsWatch, VcsWatchError
from email.utils import parseaddr
import urllib.error
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
        repo_url, branch, subpath = split_vcs_url(control["Vcs-Git"])
        return ("Git", repo_url)

    if "Vcs-Bzr" in control:
        return ("Bzr", control["Vcs-Bzr"])

    if "Vcs-Svn" in control:
        return ("Svn", control["Vcs-Svn"])

    if "Vcs-Hg" in control:
        repo_url, branch, subpath = split_vcs_url(control["Vcs-Hg"])
        return ("Hg", repo_url)

    return None, None


def verify_salsa_repository(url):
    headers = {'User-Agent': USER_AGENT}
    browser_url = determine_salsa_browser_url(url)
    response = urlopen(
        Request(browser_url, headers=headers), timeout=DEFAULT_URLLIB_TIMEOUT)
    return response.status == 200


async def retrieve_vcswatch_urls(package):
    vcs_watch = VcsWatch()
    try:
        await vcs_watch.connect()
    except ImportError:
        # No asyncpg, nothing
        raise KeyError
    return await vcs_watch.get_package(package)


class NewRepositoryURLUnknown(Exception):

    def __init__(self, vcs_type, vcs_url):
        self.vcs_type = vcs_type
        self.vcs_url = vcs_url


def find_new_urls(vcs_type, vcs_url, package, maintainer_email,
                  net_access=False):
    if net_access and (
            vcs_url.startswith('https://') or vcs_url.startswith('http://')):
        headers = {'User-Agent': USER_AGENT}
        try:
            response = urlopen(
                Request(vcs_url, headers=headers),
                timeout=DEFAULT_URLLIB_TIMEOUT)
        except urllib.error.HTTPError:
            pass
        else:
            redirected_url = response.geturl()
            if not is_on_obsolete_host(redirected_url):
                vcs_url = redirected_url
                vcs_browser = determine_browser_url(vcs_type, vcs_url)
                print("Update Vcs-* headers from URL redirect.")
                return (vcs_type, vcs_url, vcs_browser)

    # If possible, we use vcswatch to find the VCS repository URL
    if net_access:
        loop = asyncio.get_event_loop()
        try:
            (vcs_type, vcs_url, vcs_browser) = loop.run_until_complete(
                retrieve_vcswatch_urls(package))
        except VcsWatchError as e:
            sys.stderr.write('vcswatch URL unusable: %s\n' % e.args[0])
        except KeyError:
            pass
        else:
            if not is_on_obsolete_host(vcs_url):
                print("Update Vcs-* headers from vcswatch.")
                if is_on_obsolete_host(vcs_browser):
                    vcs_browser = (
                        determine_browser_url(vcs_type, vcs_url) or
                        vcs_browser)
                return (vcs_type, vcs_url, vcs_browser)
            sys.stderr.write(
                'vcswatch URL %s is still on old infrastructure.' % vcs_url)

    # Otherwise, attempt to guess based on maintainer email.
    guessed_url = guess_repository_url(package, maintainer_email)
    if guessed_url is not None:
        vcs_type = "Git"
        vcs_url = guessed_url
    else:
        vcs_url = salsa_url_from_alioth_url(vcs_type, vcs_url)
        if vcs_url is None:
            raise NewRepositoryURLUnknown(vcs_type, vcs_url)
        vcs_type = "Git"
    # Verify that there is actually a repository there
    if net_access:
        if not verify_salsa_repository(vcs_url):
            raise NewRepositoryURLUnknown(vcs_type, vcs_url)

    print("Update Vcs-* headers to use salsa repository.")

    vcs_browser = determine_salsa_browser_url(vcs_url)
    return (vcs_type, vcs_url, vcs_browser)


fixed_tags = set()


def migrate_from_obsolete_infra(control):
    vcs_type, vcs_url = get_vcs_info(control)
    if vcs_type is None:
        return
    if not is_on_obsolete_host(vcs_url):
        return

    package = control["Source"]
    maintainer_email = parseaddr(control["Maintainer"])[1]

    try:
        (vcs_type, vcs_url, vcs_browser) = find_new_urls(
            vcs_type, vcs_url, package, maintainer_email,
            net_access=(os.environ.get('NET_ACCESS', 'disallow') == 'allow'))
    except NewRepositoryURLUnknown:
        return

    fixed_tags.add("vcs-obsolete-in-debian-infrastructure")

    if "Vcs-Cvs" in control and re.match(
            r"\@(?:cvs\.alioth|anonscm)\.debian\.org:/cvsroot/",
            control["Vcs-Cvs"]):
        fixed_tags.add("vcs-field-bitrotted")

    if "Vcs-Svn" in control and "viewvc" in control["Vcs-Browser"]:
        fixed_tags.add("vcs-field-bitrotted")

    for hdr in ["Vcs-Git", "Vcs-Bzr", "Vcs-Hg", "Vcs-Svn"]:
        if hdr == "Vcs-" + vcs_type:
            continue
        try:
            del control[hdr]
        except KeyError:
            pass
    control["Vcs-" + vcs_type] = vcs_url
    if vcs_browser is not None:
        control["Vcs-Browser"] = vcs_browser
    else:
        try:
            del control["Vcs-Browser"]
        except KeyError:
            pass


update_control(source_package_cb=migrate_from_obsolete_infra)
if fixed_tags:
    print("Fixed-Lintian-Tags: " + ", ".join(sorted(fixed_tags)))
