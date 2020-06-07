#!/usr/bin/python3

import sys

from debmutate.control import ControlEditor
from lintian_brush.standards_version import (
    parse_standards_version,
    iter_standards_versions,
    )

try:
    release_dates = dict(iter_standards_versions())
except FileNotFoundError:
    sys.exit(2)


with ControlEditor() as updater:
    try:
        sv = parse_standards_version(updater.source['Standards-Version'])
    except KeyError:
        sys.exit(0)
    if sv[:3] in release_dates:
        sys.exit(0)
    if len(sv) == 2 and (sv[0], sv[1], 0) in release_dates:
        updater.source['Standards-Version'] += '.0'
        print("Add missing .0 suffix in Standards-Version.")
    elif sv > sorted(release_dates)[-1]:
        # Maybe we're just unaware of new policy releases?
        sys.exit(0)
    else:
        # Just find the previous standards version..
        candidates = [v for v in release_dates if v < sv]
        newsv = sorted(candidates)[-1]
        newsv_str = '.'.join([str(x) for x in newsv])
        print('Replace invalid standards version %s with valid %s.' % (
              updater.source['Standards-Version'], newsv_str))
        updater.source['Standards-Version'] = newsv_str


print('Fixed-Lintian-Tags: invalid-standards-version')
