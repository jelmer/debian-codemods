#!/usr/bin/python3

import asyncio
from functools import partial
from lintian_brush.changelog import ChangelogUpdater
import os
import re

certainty = 'certain'
debbugs = None


async def valid_bug(package, bug):
    if os.environ.get('NET_ACCESS', 'disallowed') == 'disallow':
        return None
    global debbugs
    if debbugs is None:
        from lintian_brush.debbugs import DebBugs
        debbugs = DebBugs()
        try:
            await debbugs.connect()
        except ImportError:
            # No asynpcg?
            return None
    return await debbugs.check_bug(package, bug)


def check_bug(package, m):
    global certainty
    bug = int(m.group('bug'))
    loop = asyncio.get_event_loop()
    valid = loop.run_until_complete(valid_bug(package, bug))
    if valid is None:
        # Let's assume valid, but downgrade certainty
        valid = True
        # Check number of digits; upstream projects don't often hit the 5-digit
        # bug numbers that Debian has.
        if len(m.group('bug')) >= 5:
            certainty = 'likely'
        else:
            certainty = 'possible'
    if valid:
        return '%s: #%d' % (m.group('closes'), bug)
    else:
        return m.group(0)


with ChangelogUpdater() as updater:
    for block in updater.changelog:
        for i, change in enumerate(block._changes):
            block._changes[i] = re.sub(
                r'(?<!partially )(?P<closes>closes) '
                r'#(?P<bug>[0-9]+)',
                partial(check_bug, block.package), change,
                flags=re.IGNORECASE)


print("Add missing colon in closes line.")
print("Fixed-Lintian-Tags: possible-missing-colon-in-closes")
print("Certainty: %s" % certainty)
