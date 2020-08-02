#!/usr/bin/python3
# Copyright (C) 2018-2020 Jelmer Vernooij <jelmer@debian.org>
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

"""Functions for working with watch files."""

import json
import os
import re
from typing import Set
from urllib.parse import urlparse, urlunparse
import urllib.error
from urllib.request import urlopen, Request

from debmutate.watch import Watch

from . import (
    USER_AGENT,
    DEFAULT_URLLIB_TIMEOUT,
    )


def candidates_from_setup_py(
        path, good_upstream_versions: Set[str], net_access=False):
    certainty = 'likely'
    from distutils.core import run_setup
    try:
        result = run_setup(os.path.abspath(path), stop_after="init")
    except BaseException:
        import traceback
        traceback.print_exc()
        return
    project = result.get_name()
    version = result.get_version()
    if not project:
        return
    current_version_filenames = None
    if net_access:
        json_url = 'https://pypi.python.org/pypi/%s/json' % project
        headers = {'User-Agent': USER_AGENT}
        try:
            response = urlopen(
                Request(json_url, headers=headers),
                timeout=DEFAULT_URLLIB_TIMEOUT)
        except urllib.error.HTTPError as e:
            if e.status == 404:
                return
            raise
        pypi_data = json.load(response)
        if version in pypi_data['releases']:
            release = pypi_data['releases'][version]
            current_version_filenames = [
                (d['filename'], d['has_sig'])
                for d in release if d['packagetype'] == 'sdist']
    filename_regex = (
        r'%(project)s-(.+)\.(?:zip|tgz|tbz|txz|(?:tar\.(?:gz|bz2|xz)))' % {
            'project': project})
    opts = []
    # TODO(jelmer): Set uversionmangle?
    # opts.append('uversionmangle=s/(rc|a|b|c)/~$1/')
    if current_version_filenames:
        for (fn, has_sig) in current_version_filenames:
            if re.match(filename_regex, fn):
                certainty = 'certain'
                if has_sig:
                    opts.append('pgpsigurlmangle=s/$/.asc/')
    url = (r'https://pypi.debian.net/%(project)s/%(fname_regex)s' % {
        'project': project, 'fname_regex': filename_regex})
    # TODO(jelmer): Add pgpsigurlmangle if has_sig==True
    w = Watch(url, opts=opts)
    yield (w, 'pypi', certainty)


def candidates_from_upstream_metadata(
        path: str, good_upstream_versions: Set[str], net_access: bool = False):
    try:
        with open(path, 'r') as f:
            inp = f.read()
    except FileNotFoundError:
        pass
    else:
        import ruamel.yaml
        code = ruamel.yaml.round_trip_load(inp, preserve_quotes=True)

        try:
            parsed_url = urlparse(code['Repository'])
        except KeyError:
            pass
        else:
            if parsed_url.hostname == 'github.com':
                yield from guess_github_watch_entry(
                    parsed_url, good_upstream_versions, net_access=net_access)


def guess_github_watch_entry(
        parsed_url, good_upstream_versions, net_access=False):
    from breezy.branch import Branch
    import re
    if not net_access:
        return
    branch = Branch.open(urlunparse(parsed_url))
    tags = branch.tags.get_tag_dict()
    POSSIBLE_PATTERNS = [r"v(\d\S+)", r"(\d\S+)"]
    version_pattern = None
    # TODO(jelmer): Maybe use releases API instead?
    # TODO(jelmer): Automatically added mangling for
    # e.g. rc and beta
    uversionmangle = []
    for name in sorted(tags, reverse=True):
        for pattern in POSSIBLE_PATTERNS:
            m = re.match(pattern, name)
            if not m:
                continue
            if m.group(1) in good_upstream_versions:
                version_pattern = pattern
                break
        if version_pattern:
            break
    else:
        return
    (username, project) = parsed_url.path.strip('/').split('/')
    if project.endswith('.git'):
        project = project[:-4]
    download_url = 'https://github.com/%(user)s/%(project)s/tags' % {
        'user': username, 'project': project}
    matching_pattern = r'.*\/%s\.tar\.gz' % version_pattern
    opts = []
    opts.append(
        r'filenamemangle=s/%(pattern)s/%(project)s-$1\.tar\.gz/' % {
            'pattern': matching_pattern,
            'project': project})
    if uversionmangle:
        opts.append(r'uversionmangle=' + ';'.join(uversionmangle))
    # TODO(jelmer): Check for GPG
    # opts.append(
    #    r'pgpsigurlmangle='
    #    r's/archive\/%s\.tar\.gz/releases\/download\/$1\/$1\.tar\.gz\.asc/' %
    #    version_pattern)
    w = Watch(download_url, matching_pattern, opts=opts)
    yield w, 'github', 'certain'


def candidates_from_hackage(
        package, good_upstream_versions, net_access=False):
    if not net_access:
        return
    url = 'https://hackage.haskell.org/package/%s/preferred' % package
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    try:
        response = urlopen(
            Request(url, headers=headers),
            timeout=DEFAULT_URLLIB_TIMEOUT)
    except urllib.error.HTTPError as e:
        if e.status == 404:
            return
        raise
    versions = json.load(response)
    for version in versions['normal-version']:
        if version in good_upstream_versions:
            break
    else:
        return
    download_url = 'https://hackage.haskell.org/package/' + package
    matching_pattern = r'.*/%s-(.*).tar.gz' % package
    w = Watch(download_url, matching_pattern)
    yield w, 'hackage', 'certain'
