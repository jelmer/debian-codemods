#!/usr/bin/python3

import email.utils

from debmutate.changelog import ChangelogEditor

from lintian_brush.fixer import LintianIssue, report_result, warn

versions = []


with ChangelogEditor() as updater:
    for block in updater.changelog:
        try:
            dt = email.utils.parsedate_to_datetime(block.date)
        except (TypeError, ValueError):
            warn(f'Invalid date {block.date!r} for {block.version}')
            # parsedate_to_datetime is buggy and raises a TypeError
            # when the date is invalid.
            continue
        if dt is None:
            # Can't interpret the date. Just ignore..
            continue
        newdate = email.utils.format_datetime(dt)
        issue = LintianIssue(
            'source', 'debian-changelog-has-wrong-day-of-week',
            info='%04d-%02d-%02d is a %s' % (
                dt.year, dt.month, dt.day, dt.strftime('%A')))
        if newdate[:3] != block.date[:3] and issue.should_fix():
            block.date = newdate
            versions.append(block.version)
            issue.report_fixed()

if len(versions) == 1:
    report_result(
        'Fix day-of-week for changelog entry %s.'
        % ', '.join([str(v) for v in versions]))
else:
    report_result(
        'Fix day-of-week for changelog entries %s.'
        % ', '.join([str(v) for v in versions]))
