#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij
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

import glob
from setuptools import setup

setup(
    name="lintian-brush",
    version="0.48",
    author="Jelmer Vernooij",
    author_email="jelmer@debian.org",
    packages=["lintian_brush"],
    url="https://salsa.debian.org/jelmer/lintian-brush",
    description="Automatic lintian issue fixer",
    project_urls={
        "Repository": "https://salsa.debian.org/jelmer/lintian-brush",
    },
    requires=['breezy', 'debian'],
    entry_points={
        'console_scripts': [
            'lintian-brush=lintian_brush.__main__:main',
            'apply-multiarch-hints=lintian_brush.multiarch_hints:main',
            ]
    },
    test_suite='lintian_brush.tests.test_suite',
    data_files=[
        ('share/lintian-brush/fixers', glob.glob('fixers/*')),
        ('share/lintian-brush', ['spdx.json']),
    ],
)
