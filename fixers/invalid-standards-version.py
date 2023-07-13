#!/usr/bin/python3

import sys

from debmutate.control import parse_standards_version

from lintian_brush.fixer import LintianIssue, control, report_result
from lintian_brush.standards_version import iter_standards_versions

try:
    release_dates = dict(iter_standards_versions())
except FileNotFoundError:
    sys.exit(2)


with control as updater:
    try:
        sv = parse_standards_version(updater.source['Standards-Version'])
    except KeyError:
        sys.exit(0)
    if (sv in release_dates or
            sv[:4] in release_dates or
            len(sv) == 3 and
            sv + (0, ) in release_dates):
        sys.exit(0)
    invalid_version = updater.source['Standards-Version']
    issue = LintianIssue(
        'source', 'invalid-standards-version', invalid_version)
    if issue.should_fix():
        issue.report_fixed()
        if len(sv) == 2 and (sv[0], sv[1], 0, 0) in release_dates:
            updater.source['Standards-Version'] += '.0'
            report_result("Add missing .0 suffix in Standards-Version.")
        elif sv > sorted(release_dates)[-1]:
            # Maybe we're just unaware of new policy releases?
            sys.exit(0)
        else:
            # Just find the previous standards version..
            candidates = [v for v in release_dates if v < sv]
            newsv = sorted(candidates)[-1]
            newsv_str = '.'.join([str(x) for x in newsv])
            report_result(
                'Replace invalid standards version {} with valid {}.'.format(
                    updater.source['Standards-Version'], newsv_str))
            updater.source['Standards-Version'] = newsv_str
