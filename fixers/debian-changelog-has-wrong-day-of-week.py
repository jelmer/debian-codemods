#!/usr/bin/python3

import email.utils
from lintian_brush.changelog import update_changelog

versions = []


def fix_dow(block):
    try:
        dt = email.utils.parsedate_to_datetime(block.date)
    except TypeError:
        # parsedate_to_datetime is buggy and raises a TypeError
        # when the date is invalid.
        return
    if dt is None:
        # Can't interpret the date. Just ignore..
        return
    newdate = email.utils.format_datetime(dt)
    if newdate[:3] != block.date[:3]:
        block.date = newdate
        versions.append(block.version)


update_changelog(block_cb=fix_dow)

print('Fix day-of-week for changelog entries %s.'
      % ', '.join([str(v) for v in versions]))
print('Fixed-Lintian-Tags: debian-changelog-has-wrong-day-of-week')
