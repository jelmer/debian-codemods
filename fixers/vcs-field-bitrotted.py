#!/usr/bin/python3

import asyncio
import os
import re
from lintian_brush import USER_AGENT
from lintian_brush.control import update_control
from lintian_brush.salsa import (
    determine_browser_url,
    guess_repository_url,
    salsa_url_from_alioth_url,
    )
from lintian_brush.vcswatch import VcsWatch
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
        return ("Git", control["Vcs-Git"].split(' ')[0])

    if "Vcs-Bzr" in control:
        return ("Bzr", control["Vcs-Bzr"])

    if "Vcs-Svn" in control:
        return ("Svn", control["Vcs-Svn"])

    if "Vcs-Hg" in control:
        return ("Hg", control["Vcs-Hg"])

    return None, None


def verify_salsa_repository(url):
    headers = {'User-Agent': USER_AGENT}
    browser_url = determine_browser_url(url)
    response = urlopen(Request(browser_url, headers=headers))
    return response.status == 200


async def retrieve_vcswatch_urls(package):
    vcs_watch = VcsWatch()
    try:
        await vcs_watch.connect()
    except ImportError:
        # No asyncpg, nothing
        raise KeyError
    return await vcs_watch.get_package(package)


fixed_tags = set()


def migrate_from_obsolete_infra(control):
    vcs_type, vcs_url = get_vcs_info(control)
    if vcs_type is None:
        return
    if not is_on_obsolete_host(vcs_url):
        return

    package = control["Source"]
    maintainer_email = parseaddr(control["Maintainer"])[1]

    # If possible, we use vcswatch to find the VCS repository URL
    loop = asyncio.get_event_loop()
    try:
        if os.environ.get('VCSWATCH', 'enabled') == 'enabled':
            (vcs_type, vcs_url, vcs_browser) = loop.run_until_complete(
                retrieve_vcswatch_urls(package))
        else:
            raise KeyError
        print("Update Vcs-* headers from vcswatch.")
        fixed_tags.add("vcs-obsolete-in-debian-infrastructure")
    except KeyError:
        # Otherwise, attempt to guess based on maintainer email.
        guessed_url = guess_repository_url(package, maintainer_email)
        if guessed_url is not None:
            vcs_type = "Git"
            vcs_url = guessed_url
        else:
            vcs_url = salsa_url_from_alioth_url(vcs_type, vcs_url)
            if vcs_url is None:
                return
            vcs_type = "Git"
        # Verify that there is actually a repository there
        if os.environ.get('SALSA_PROBE', 'enabled') == 'ensabled':
            if not verify_salsa_repository(guessed_url):
                return
        print("Update Vcs-* headers to use salsa repository.")
        fixed_tags.add("vcs-obsolete-in-debian-infrastructure")

        vcs_browser = determine_browser_url(vcs_url)

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
        del control["Vcs-Browser"]


update_control(source_package_cb=migrate_from_obsolete_infra)
if fixed_tags:
    print("Fixed-Lintian-Tags: " + ", ".join(sorted(fixed_tags)))
