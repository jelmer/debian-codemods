#!/usr/bin/python
# Copyright (C) 2019 Jelmer Vernooij
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

"""Lintian-brush configuration file."""

import os
from typing import Optional
import warnings

from configobj import ConfigObj
import distro_info


PACKAGE_CONFIG_FILENAME = "debian/lintian-brush.conf"


SUPPORTED_KEYS = [
    "compat-release",
    "minimum-certainty",
    "allow-reformatting",
    "update-changelog",
]


def _oldoldstable(debian_info):
    distros = [x for x in debian_info._releases if x.release is not None]
    if len(distros) < 2:
        raise distro_info.DistroDataOutdated()
    return distros[-3].series


def _oldest_name(fn):
    return min(fn(result="object"), key=lambda r: r.created).series


def oldest_supported_lts(info):
    return min(
        [r for r in info.supported(result='object')
         if info.is_lts(r.series)],
        key=lambda r: r.created).series


def resolve_release_codename(name: str, date=None) -> Optional[str]:
    if '/' in name:
        distro, name = name.split('/', 1)
    else:
        distro = None
    if distro in ('debian', None):
        debian = distro_info.DebianDistroInfo()
        if name == 'lts':
            return _oldest_name(debian.lts_supported)
        if name == 'elts':
            return _oldest_name(debian.elts_supported)
        if name == 'oldoldstable':
            return _oldoldstable(debian)
        if debian.codename(name):
            return debian.codename(name)
        if debian.valid(name):
            return name
    if distro in ('ubuntu', None):
        ubuntu = distro_info.UbuntuDistroInfo()
        if name == 'esm':
            return _oldest_name(ubuntu.supported_esm)
        if name == 'lts':
            return oldest_supported_lts(ubuntu)
        if ubuntu.valid(name):
            return name
        return None
    return None


class Config(object):
    """A configuration file."""

    def __init__(self, path):
        if not os.path.exists(path):
            raise FileNotFoundError(path)
        self._obj = ConfigObj(path, raise_errors=True, file_error=True)
        for k in self._obj.keys():
            if k not in SUPPORTED_KEYS:
                warnings.warn("unknown setting %s in %s" % (k, path))

    @classmethod
    def from_workingtree(cls, wt, subpath):
        return cls(os.path.join(wt.basedir, subpath, PACKAGE_CONFIG_FILENAME))

    def compat_release(self):
        value = self._obj.get("compat-release")
        codename = resolve_release_codename(value)
        if codename is None:
            warnings.warn("unknown compat release %s, ignoring." % value)
        return codename

    def allow_reformatting(self):
        try:
            return self._obj.as_bool("allow-reformatting")
        except KeyError:
            return None

    def minimum_certainty(self):
        return self._obj.get("minimum-certainty")

    def update_changelog(self):
        try:
            return self._obj.as_bool("update-changelog")
        except KeyError:
            return None
