#!/usr/bin/python3

import re
import socket
import urllib.error
from contextlib import suppress
from email.utils import parseaddr
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from debmutate.vcs import (
    get_vcs_info,
)

from lintian_brush import DEFAULT_URLLIB_TIMEOUT, USER_AGENT
from lintian_brush.fixer import (
    control,
    fixed_lintian_tag,
    net_access_allowed,
    report_result,
    warn,
)
from lintian_brush.salsa import (
    determine_browser_url as determine_salsa_browser_url,
)
from lintian_brush.salsa import (
    guess_repository_url,
    salsa_url_from_alioth_url,
)
from lintian_brush.vcs import (
    determine_browser_url,
)
from lintian_brush.vcswatch import VcsWatch, VcsWatchError

OBSOLETE_HOSTS = [
    'anonscm.debian.org', 'alioth.debian.org', 'svn.debian.org',
    'git.debian.org', 'bzr.debian.org', 'hg.debian.org']


def is_on_obsolete_host(url):
    parsed_url = urlparse(url)
    host = parsed_url.netloc.split('@')[-1]
    return host in OBSOLETE_HOSTS


def verify_salsa_repository(url):
    headers = {'User-Agent': USER_AGENT}
    browser_url = determine_salsa_browser_url(url)
    try:
        response = urlopen(
            Request(browser_url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except socket.timeout:
        return None
    return response.status == 200


def retrieve_vcswatch_urls(package):
    try:
        with VcsWatch() as vcs_watch:
            return vcs_watch.get_package(package)
    except ImportError as exc:
        # No psycopg2, nothing
        raise KeyError(package) from exc


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
        except (urllib.error.HTTPError, urllib.error.URLError):
            pass
        except socket.timeout:
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
        try:
            (vcs_type, vcs_url, vcs_browser) = retrieve_vcswatch_urls(package)
        except VcsWatchError as e:
            warn('vcswatch URL unusable: %s' % e.args[0])
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
            warn('vcswatch URL %s is still on old infrastructure.' % vcs_url)

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
    if net_access and verify_salsa_repository(vcs_url) is False:
        raise NewRepositoryURLUnknown(vcs_type, vcs_url)

    print("Update Vcs-* headers to use salsa repository.")

    vcs_browser = determine_salsa_browser_url(vcs_url)
    return (vcs_type, vcs_url, vcs_browser)


def migrate_from_obsolete_infra(control):
    vcs_type, vcs_url, unused_subpath = get_vcs_info(control)
    if vcs_type is None:
        return
    if not is_on_obsolete_host(vcs_url):
        return

    package = control["Source"]
    maintainer_email = parseaddr(control["Maintainer"])[1]

    old_vcs_browser = control.get('Vcs-Browser')
    old_vcs_type = vcs_type
    old_vcs_url = vcs_url
    try:
        (vcs_type, vcs_url, vcs_browser) = find_new_urls(
            vcs_type, vcs_url, package, maintainer_email,
            net_access=net_access_allowed())
    except NewRepositoryURLUnknown:
        return

    fixed_lintian_tag(
        'source', "vcs-obsolete-in-debian-infrastructure",
        info=f'vcs-{old_vcs_type.lower()} {old_vcs_url}')

    if (("Vcs-Cvs" in control and re.match(
            r"\@(?:cvs\.alioth|anonscm)\.debian\.org:/cvsroot/",
            control["Vcs-Cvs"])) or
        ("Vcs-Svn" in control and
            "viewvc" in control["Vcs-Browser"])):
        fixed_lintian_tag(
            'source', "vcs-field-bitrotted",
            info='{} {}'.format(old_vcs_url or '', old_vcs_browser or ''))

    for hdr in ["Vcs-Git", "Vcs-Bzr", "Vcs-Hg", "Vcs-Svn"]:
        if hdr == "Vcs-" + vcs_type:  # type: ignore
            continue
        with suppress(KeyError):
            del control[hdr]
    control["Vcs-" + vcs_type] = vcs_url  # type: ignore
    if vcs_browser is not None:
        control["Vcs-Browser"] = vcs_browser
    else:
        with suppress(KeyError):
            del control["Vcs-Browser"]


with control as updater:
    migrate_from_obsolete_infra(updater.source)

report_result()
