#!/usr/bin/python3

import sys

from lintian_brush.control import update_control


def parse_version(v):
    return tuple([int(k) for k in v.split('.')])


RELEASE_DATES_PATH = '/usr/share/lintian/data/standards-version/release-dates'

release_dates = {}
try:
    with open(RELEASE_DATES_PATH, 'r') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            (version, ts) = line.split()
            release_dates[parse_version(version)] = ts
except FileNotFoundError:
    sys.exit(2)


def fix_invalid_standards_version(control):
    try:
        sv = parse_version(control['Standards-Version'])
    except KeyError:
        return
    if sv[:3] in release_dates:
        return
    if len(sv) == 2 and (sv[0], sv[1], 0) in release_dates:
        control['Standards-Version'] += '.0'
        print("Add missing .0 suffix in Standards-Version.")
        return
    if sv > sorted(release_dates)[-1]:
        # Maybe we're just unaware of new policy releases?
        return
    # Just find the previous standards version..
    candidates = [v for v in release_dates if v < sv]
    newsv = sorted(candidates)[-1]
    newsv_str = '.'.join([str(x) for x in newsv])
    print('Replace invalid standards version %s with valid %s.' % (
          control['Standards-Version'], newsv_str))
    control['Standards-Version'] = newsv_str


update_control(source_package_cb=fix_invalid_standards_version)
print('Fixed-Lintian-Tags: invalid-standards-version')
