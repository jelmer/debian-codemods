#!/usr/bin/python3
# Copyright (C) 2018 Jelmer Vernooij <jelmer@debian.org>
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

import os
import argparse
import subprocess
import sys
from breezy.workingtree import WorkingTree
import locale
locale.setlocale(locale.LC_ALL, '')
# Use better default than ascii with posix filesystems that deal in bytes
# natively even when the C locale or no locale at all is given. Note that
# we need an immortal string for the hack, hence the lack of a hyphen.
sys._brz_default_fs_enc = "utf8"

import breezy
breezy.initialize()
import breezy.git
import breezy.bzr

from . import (
    available_lintian_fixers,
    run_lintian_fixers,
    version_string,
    )

parser = argparse.ArgumentParser(prog='lintian-brush')
parser.add_argument('--no-update-changelog', action="store_true", help="Whether to update the changelog.")
parser.add_argument('--version', action='version', version='%(prog)s ' + version_string)
parser.add_argument('--list', action="store_true", help="List available fixers.")
parser.add_argument('fixers', metavar='TAGS', nargs='*', help='Lintian tag for which to apply fixer.')
args = parser.parse_args()

wt = WorkingTree.open('.')
fixers = available_lintian_fixers()
if args.list:
    for fixer in sorted([fixer.tag for fixer in fixers]):
        print(fixer)
else:
    if args.fixers:
        fixers = [f for f in fixers if f.tag in args.fixers]
    with wt.lock_write():
        run_lintian_fixers(wt, fixers, update_changelog=(not args.no_update_changelog))
