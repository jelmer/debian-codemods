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
import breezy.plugins.launchpad
import breezy.plugins.debian # for apt: urls
from breezy.trace import note

from . import (
    available_lintian_fixers,
    run_lintian_fixers,
    )

parser = argparse.ArgumentParser()
parser.add_argument('--no-update-changelog', action="store_true", help="Whether to update the changelog.")
parser.add_argument('fixers', metavar='TAGS', nargs='*', help='Lintian tag for which to apply fixer.')
args = parser.parse_args()

wt = WorkingTree.open('.')
fixers = available_lintian_fixers()
if args.fixers:
    fixers = [f for f in fixers if f.tag in args.fixers]
with wt.lock_write():
    run_lintian_fixers(wt, fixers, update_changelog=(not args.no_update_changelog))
