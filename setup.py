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
from setuptools_rust import Binding, RustBin, RustExtension

setup(
    data_files=[
        (
            "share/lintian-brush/fixers",
            [n for n in glob.glob("fixers/*") if not n.endswith("/slow")],
        ),
        (
            "share/lintian-brush",
            [
                "spdx.json",
                "renamed-tags.json",
                "key-package-versions.json",
            ],
        ),
    ],
    rust_extensions=[
        RustBin("debianize", "debianize/Cargo.toml"),
        RustBin("lintian-brush", "lintian-brush/Cargo.toml"),
        RustBin("detect-changelog-behaviour", "analyzer/Cargo.toml", features=["cli"]),
        RustBin("deb-vcs-publish", "analyzer/Cargo.toml", features=["cli"]),
        RustBin("dump-multiarch-hints", "multiarch-hints/Cargo.toml"),
        RustBin(
            "apply-multiarch-hints",
            "multiarch-hints/Cargo.toml",
            features=["cli"],
        ),
        RustExtension(
            "lintian_brush._lintian_brush_rs",
            "lintian-brush-py/Cargo.toml",
            binding=Binding.PyO3,
            features = ["extension-module"]
        ),
        RustExtension(
            "lintian_brush._debianize_rs",
            "debianize-py/Cargo.toml",
            binding=Binding.PyO3,
        ),
    ],
)
