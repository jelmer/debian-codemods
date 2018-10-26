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

import argparse
import sys
from breezy.workingtree import WorkingTree
import locale
locale.setlocale(locale.LC_ALL, '')
# Use better default than ascii with posix filesystems that deal in bytes
# natively even when the C locale or no locale at all is given. Note that
# we need an immortal string for the hack, hence the lack of a hyphen.
sys._brz_default_fs_enc = "utf8"

import breezy  # noqa: E402
breezy.initialize()
import breezy.git  # noqa: E402
import breezy.bzr  # noqa: E402

from . import (  # noqa: E402
    available_lintian_fixers,
    find_fixers_dir,
    run_lintian_fixers,
    version_string,
    )

parser = argparse.ArgumentParser(prog='lintian-brush')
parser.add_argument(
    '--no-update-changelog', action="store_true",
    help="Whether to update the changelog.")
parser.add_argument(
    '--version', action='version', version='%(prog)s ' + version_string)
parser.add_argument(
    '--list-fixers', action="store_true", help="List available fixers.")
parser.add_argument(
    '--list-tags', action="store_true",
    help="List lintian tags for which fixers are available.")
parser.add_argument(
    '--fixers-dir', type=str, help='Path to fixer scripts. [%(default)s]',
    default=find_fixers_dir())
parser.add_argument(
    'fixers', metavar='TAGS', nargs='*',
    help='Lintian tag for which to apply fixer.')
args = parser.parse_args()

if args.list_fixers and args.list_tags:
    parser.print_usage()
    sys.exit(1)

wt = WorkingTree.open('.')
fixers = available_lintian_fixers(args.fixers_dir)
if args.list_fixers:
    for script in sorted([fixer.script_path for fixer in fixers]):
        print(script)
elif args.list_tags:
    tags = set()
    for fixer in fixers:
        tags.update(fixer.lintian_tags)
    for tag in sorted(tags):
        print(tag)
else:
    if args.fixers:
        fixers = [f for f in fixers if f.tag in args.fixers]
    with wt.lock_write():
        run_lintian_fixers(
            wt, fixers, update_changelog=(not args.no_update_changelog))
