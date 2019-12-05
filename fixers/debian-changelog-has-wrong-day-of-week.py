#!/usr/bin/python3

import email.utils
from lintian_brush.changelog import ChangelogUpdater

versions = []


with ChangelogUpdater() as updater:
    for block in updater.changelog:
        try:
            dt = email.utils.parsedate_to_datetime(block.date)
        except TypeError:
            # parsedate_to_datetime is buggy and raises a TypeError
            # when the date is invalid.
            continue
        if dt is None:
            # Can't interpret the date. Just ignore..
            continue
        newdate = email.utils.format_datetime(dt)
        if newdate[:3] != block.date[:3]:
            block.date = newdate
            versions.append(block.version)

if len(versions) == 1:
    print('Fix day-of-week for changelog entry %s.'
          % ', '.join([str(v) for v in versions]))
else:
    print('Fix day-of-week for changelog entries %s.'
          % ', '.join([str(v) for v in versions]))
print('Fixed-Lintian-Tags: debian-changelog-has-wrong-day-of-week')
