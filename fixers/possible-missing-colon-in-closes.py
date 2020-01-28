#!/usr/bin/python3

import asyncio
from functools import partial
from lintian_brush.changelog import ChangelogUpdater
from lintian_brush.fixer import net_access_allowed
import re
import socket
from warnings import warn

certainty = 'certain'
debbugs = None
tags = set()


async def valid_bug(package, bug):
    if not net_access_allowed():
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
        except socket.gaierror as e:
            warn('Unable to connect to debbugs: %s' % e)
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
        tags.add("possible-missing-colon-in-closes")
        return '%s: #%d' % (m.group('closes'), bug)
    else:
        return m.group(0)


def fix_close_typo(m):
    tags.add('misspelled-closes-bug')
    return '%ss: #%s' % (m.group('close'), m.group('bug'))


with ChangelogUpdater() as updater:
    for block in updater.changelog:
        for i, change in enumerate(block._changes):
            change = re.sub(
                r'(?<!partially )(?P<closes>closes) '
                r'#(?P<bug>[0-9]+)',
                partial(check_bug, block.package), change,
                flags=re.IGNORECASE)
            change = re.sub(
                '(?P<close>close): #(?P<bug>[0-9]+)',
                fix_close_typo, change, flags=re.IGNORECASE)
            block._changes[i] = change


if tags == set(['possible-missing-colon-in-closes']):
    print("Add missing colon in closes line.")
elif tags == set(['misspelled-closes-bug']):
    print("Fix misspelling of Close => Closes.")
else:
    print("Fix formatting of bug closes.")
print("Fixed-Lintian-Tags: %s" % ", ".join(sorted(tags)))
print("Certainty: %s" % certainty)
