#!/usr/bin/python3

from lintian_brush.changelog import ChangelogUpdater
import re


def check_bug(m):
    bug = int(m.group(2))
    return '%s: #%d' % (m.group(1), bug)


with ChangelogUpdater() as updater:
    for block in updater.changelog:
        for i, change in enumerate(block._changes):
            block._changes[i] = re.sub(
                r'(closes) #([0-9]+)',
                check_bug, change,
                flags=re.IGNORECASE)


print("Add missing colon in closes line.")
print("Fixed-Lintian-Tags: possible-missing-colon-in-closes")
